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

### C. elegans closed-loop behaviour evidence

Question tested: can the preserved `network_celegans.json` make the MuJoCo worm
sense, wriggle, move and explore? Exchange counts cannot answer this, so the
native adapter gained two opt-in options. `--trace DIR [--trace-every N]` writes
`DIR/INSTANCE.jsonl`: segment positions, spine angles, muscle targets, submitted
sensors and cumulative applied muscle spikes. It is read-only; a regression test
and per-step silent/NPC runs show bit-identical physics. An unbound, non-NPC worm
also records its stateless sensors as a passive control. Headless `--lockstep`
waits for the reply to each capture before the next 2 ms physics step. This
replaces wall-clock pacing, and the 5 s data fault and 100 ms expiry still apply.

```bash
export AARNN_MUJOCO_DIR=/home/pbisaacs/Developer/mujoco-3.17.0
cargo xtask qa run --suite lec-crow-mujoco-behaviour   # 30 s lock-step + passive control
# Direct probe; --pacing realtime reproduces the default launcher clock.
python3 scripts/qa/probe_celegans_mujoco_behaviour.py --output target/qa/new-dir \
  --steps 15000 --pacing lockstep
```

The probe fixes its thresholds before running. It writes `behaviour.json` and
`behaviour.png` with the trajectory, kymograph, muscle drive and sensors. The
2026-11-20 evidence is in `target/qa/celegans-behaviour/`:

| Run | Brain frames applied | Applied muscle spikes | Wriggling joints | Net move | Result |
|---|---|---|---|---|---|
| Real time, 60 s (`run-60s`) | 511 applied, 156 expired, 29,331 gaps | 642 | 0/23 | 2.2 cm | sense only |
| Lock-step, 30 s (`lockstep-30s`) | 15,000/15,000, 0 expired | 10,034 | 0/23 (max 0.004 rad) | 2.4 cm | sense only |
| Passive control, same binary | none | 0 | 0/23 | 2.5 cm (settling) | — |
| Signed candidate, lock-step 3 s (`pilot-signed`) | 1,500/1,500 | 1,244 | 0/23 | 2.8 cm | sense only |

**Result:** the claim is not supported. Sensing works: 16 channels vary,
including light, taste, chemical-proxy and far-range rays. Every lock-step frame
reaches the worker. Muscle output does not alternate: dorsal/ventral spike-rate
correlation is +0.86 to +0.93. Across all channels, mean pairwise correlation
is about +0.22. Half the spikes occur in 50 ms bins where more than 30% of the
96 channels fire together. `ventral − dorsal` therefore cancels, the spine never
reverses and displacement equals passive settling. Measured causes:

1. **Timing starvation (real time).** The CPU-reference brain takes about 50 ms
   per 2 ms frame. It receives about 1 in 45 frames and advances about 2.4 s
   while the body advances 60 s. The distributed shadow runner lowers AARNN
   depth to 0 under its 10 ms step target. Lock-step removes this cause but
   does not change the result.
2. **Dropped inhibition.** `EDGE_RE` in `scripts/build_celegans_network_json.py`
   matches only `[0-9]+`. All 125 negative `celegans.py` neuron-to-muscle pairs
   (Σ −860), including every GABAergic DD/VD synapse, are absent. This matches
   `docs/research/poster_v2/FACTS.md`. Do not edit the importer in place:
   `scripts/run_multi_robot_webots.sh` regenerates `network_celegans.json` when
   that script is newer.
3. **Inverted sign heuristic.** `Runner::is_inhibitory_type_name` treats every
   type containing `interneuron` as inhibitory. Strictness 0.9 therefore makes
   all 103 worm interneurons inhibitory, including AVA/AVB/AVD/AVE/PVC command
   cells. GABAergic DD/VD cells, typed `Motor`, become excitatory.
4. **Separate motor runner.** In managed IPC mode, `App::new`'s local Runner
   steps each admitted frame and produces the motor reply. The distributed
   node's Runner consumes the same frame separately and does not drive muscles.

An uncommitted, minimal candidate in `target/qa/celegans-behaviour/candidate/`
added the 125 signed synapses, set Dale strictness to 0 and disabled import
rewire. It was selected through `--network-map`/`--config-map`. Its motor
pattern was unchanged, so restoring signs alone is insufficient. Global
synchronous output and the lack of body-bend proprioception among the 24 inputs
remain open modelling questions. No preserved network, configuration or world
changed. These results do not establish biological adequacy.

### C. elegans import v2, biological I/O, habitat v3 and depth study (2026-11-21)

The earlier evidence is superseded: `network_celegans.json` is regenerated by
the corrected importer (the 2026-09-17 file is not reused), and the MuJoCo
worm uses a 48-input contract.

**Import (`scripts/build_celegans_network_json.py`, `scripts/celegans_anatomy.py`).**
All 3,689 signed `celegans.py` pairs are imported with exact counts
(`w = signed count / 37`, so |w| <= 1 and no bound clips a value); the
commented-out DVB->MANAL line is not a synapse, so the 96-muscle order is
unchanged. `scripts/qa/test_celegans_import.py` re-parses the source
independently and checks every pair. Other values the old import lost or
rewrote:

| Defect | Fix |
|---|---|
| `EDGE_RE` dropped 125 negative neuromuscular pairs | signed pattern; `--polarity transmitter` (default) also makes the 141 chemical outputs of the 26 GABAergic cells inhibitory (`--polarity source` keeps source signs) |
| Prefix typing made ASE/ASG/ASH/ASI/ASJ/ASK `Motor` in the ventral cord, ALM a head cell, SMD/RMD cord cells | 302-cell anatomy table: modality, transmitter, ganglion, soma position |
| `Interneuron` => inhibitory heuristic (all 103 interneurons inhibitory, DD/VD excitatory) | transmitter-qualified types (`Interneuron:acetylcholine`, `Motor:GABA`); `is_inhibitory_type_name` honours the qualifier |
| Per-step Dale enforcement clamped every weight to +-1 and re-signed it | signs live in the matrices; `aarnn_dale_strictness = 0` |
| Import rewire kept 74 % of synapses | off |
| Per-step synaptic scaling (3 % per 2 ms) equalised every cell's input | off (homeostatic scaling acts over hours) |
| Growth, spontaneous neurons, pruning after 30 s, sleep replay of the first 1.2 s | adult connectome frozen: growth/pruning/sleep off, stage pinned |
| Partial per-type `bio_params` fell back to code defaults | complete parameter sets per type |
| No electrical synapses (Busbice source is chemical only) | 1,080 gap-junction pairs from Cook et al. 2019 (`data/celegans/herm_full_edgelist.csv`, OpenWorm c302, sha256 recorded), 0.025 per contact, integrated implicitly; proximity mean-field coupling off |
| Morphology regrown with unseeded thread RNGs on every load | snapshot `rng_seed`; `--export-frozen` persists one anatomy for all variants |

**Runtime calculation errors fixed (`src/runner.rs`, `src/neuron_kernels.rs`,
`src/aarnn/*.rs`, `src/cl_compute.rs`)**, each with a regression test:

| Level | Error | Fix |
|---|---|---|
| all | plasticity commits clamped signed weights to `[w_min, w_max] = [0, 1]` every step (inhibition erased even at zero delta); bouton overlay likewise | sign-preserving magnitude bounds; `aarnn_plasticity_enabled` switch |
| all | output gain floor rescaled all 96 muscle currents to a peak of 8 (up to 512x) whenever any cell spiked | `aarnn_output_gain_floor_enabled` switch (off for the worm) |
| all | membranes started at 0 mV: every cell fired synchronously after build/import/reset | Izhikevich stable fixed point |
| all | one 1 ms forward-Euler step (Izhikevich used 0.5 ms) | <= 0.5 ms sub-steps, stop at the spike crossing (CPU and both OpenCL kernels) |
| all | layer 0 and outputs ignored per-type presets/adaptive parameters | per-neuron parameters |
| all | parallel layer-0 fallback path had no recurrent current (serial path did) | identical paths; 1-thread and 8-thread runs are bit-identical |
| all | untyped imported cells inherited the previous network's per-cell biology | every cell reset to the imported defaults |
| all | `ensure_output_connectivity` added an excitatory edge to inhibition-only muscle rows | any non-zero weight counts |
| all | hashed Dale fallback inverted ~36 % of sensory channels | typed sensory channels use their types |
| 0 | a spike delivered `w dt` charge, ~14x less than the same weight at depth >= 1 | same charge as the AMPA/NMDA/GABA kernel integral, without its time course |
| >= 1 | STP release of the newest frame scaled spikes read `steps` frames back: routed recurrent transmission was exactly zero with refractoriness | release history aligned with the spike history |
| >= 1 | STP scaled every synapse by `U` (0.2) at rest | release relative to rest (`u x / U`); inert on transducer channels |
| >= 1 | route delays: bouton latency rounded to 0 at dt = 1 ms; history length from straight-line distance clamped every route delay | one conservative quantisation per route; history sized from route delays; re-quantised on depth change |
| >= 1 | hidden layers >= 1 discarded distance attenuation | applied |
| >= 2 | adaptive threshold only moved the +30 mV peak test (no effect on rate) | subtractive adaptation current |
| >= 1 | dopamine/serotonin ratio could multiply eta by 1e6 | bounded |

Documented approximations left in place (hardware-facing FPAA reference
models depend on them): the synaptic filter routes the *net* current of a step
(E and I cancel before filtering) and the NMDA gate is sampled at injection.
Depths 4 and 5 are bit-identical to depth 3 (tested).

**Biological I/O (`scripts/embodiment_legacy.py`, `adapters/mujoco/src/world.rs`,
`crates/aarnn-embodiment/src/celegans.rs`).** Muscle response v3: row 01 is
the head (v2 indexed rows from the tail, so head motor output bent the tail and
row 01 drove nothing); each hinge is driven by the two rows spanning it;
MVULVA neither bends the body nor updates 23 times per step; twitch summation
(0.3 per spike, 55 ms) replaces the binary-frame low-pass that needed 500 Hz for
full contraction. Sensors v2 sample analytic stimulus fields at the amphid
(nose) and phasmid (tail) mounts and drive the neurons that transduce them:
touch (ALM/AVM/PLM, nose touch ASH/FLP/OLQ/IL1), plate taps on high-passed
acceleration (TRNs), light (ASJ/ASK/ASH/AWB/ASI), temperature (AFD/AWC/ASI),
soluble attractant (ASE/ASG/ASI/ADF), soluble repellent (ASH/ADL nose, PHA/PHB
tail), food odour (AWA/AWC), O2 (URX/AQR, PQR), proximity rays that no longer
hit the worm's own body, and 24 rectified dorsal/ventral bend channels feeding
B-type motor neurons (anterior receptive field), SMD/RMD and DVA. The Webots
route keeps a 24-channel build (`network_celegans_webots.json`).

**Habitat v3 (`scripts/embodiment_scene.py`).** Odour plumes and soluble food
on three lawns, salt spot, repellent spot and copper ring, plate temperature
gradient with warm patch and cool refuge, light spot, O2 depleted in lawns,
plate taps every 15 s, and a resistive-force agar substrate (normal/tangential
drag ratio 20) applied identically to the passive control.

**Lock-step video.** Lock-step runs record full `qpos` and muscle activations
(`aarnn-mujoco-trace-v2`); `scripts/qa/render_celegans_trace_video.py` restores
each recorded state (no re-simulation) and encodes 25 frames per simulated
second, so the MP4 replays at true simulation speed without lag. Cuticle tint
shows ventral (red) or dorsal (blue) drive; the trail shows the head path.

```bash
python3 scripts/build_celegans_network_json.py --config-out webots_world/configs/config_celegans_mujoco.json
cargo build --release --features growth3d,morpho --example celegans_depth_probe
target/release/examples/celegans_depth_probe --network network_celegans.json --export-frozen network_celegans_frozen.json
AARNN_MUJOCO_DIR=$HOME/Developer/mujoco-3.17.0 python3 scripts/qa/celegans_depth_study.py \
  --output target/qa/celegans-depth-study --closed-loop --open-loop
```

**First depth study** (`target/qa/celegans-depth-study/`, superseded below):
every level was numerically sound, but the worm only twitched (curvature
kL 0.3-0.5 against 3-9 in worms, D/V correlation +0.05, depth 0 rigid
co-contraction at +0.78).

**Follow-up: why the body could not undulate, and what changed.**

- *Mechanics (the decisive limit).* With a programmed biological wave (0.5 Hz,
  0.65 L, 0.3 rad) the v2 body tracked 28 % of the commanded bend: hinge
  damping 0.02 N m s/rad exceeded the 6 mN m servo at 0.6 rad/s. MuJoCo also
  mixes contact friction by maximum, so the ground's 0.8 overrode the cuticle
  and isotropic Coulomb friction swamped the drag anisotropy (speed was
  independent of the drag ratio). Mechanics v3: damping 0.002, servo kp 0.2 /
  30 mN m, cuticle friction 0.02 with contact priority, resistive drag
  0.01/0.2 N s/m. The programmed wave now tracks at 99 % and crawls at
  0.16 L/s (worm 0.1-0.2); the reversed wave crawls backward
  (`celegans_travelling_wave_crawls_in_the_wave_direction`).
- *Electrical synapses.* Coupling is integrated implicitly (backward Euler,
  conjugate-gradient solve of `(I + dt L) v_c = v`): stable for any
  conductance, current-conserving, equal to ohmic for weak coupling. The
  explicit 0.0039/contact cap is gone; 0.025/contact gives a 10-contact pair a
  coupling coefficient of about 0.3.
- *Sensory drive.* Tonic fields drove sensory cells at 100-244 Hz and the
  App encoder added random spikes on 1-8 % of channels every 2 ms. Now
  exteroceptive transducers adapt (depressing transmission, U 0.3, 1.5 s
  recovery), bend proprioceptors stay tonic, the App passes the client's
  AER spikes through (`threshold` strategy), and vibration/rotation are scaled
  so the worm's own wriggle no longer reads as a plate tap (taps raised to
  40 m/s^2). Replayed closed-loop input now gives sensory cells ~5 Hz at
  steady state.
- *Head oscillator.* SMD head proprioception is a stretch reflex (Yeon et al.
  2018: a ventral head bend stretches the dorsal side and excites SMDD), i.e.
  delayed negative feedback; the earlier mapping was same-side positive
  feedback. `--head-proprio-gain`, `--head-extent`, `--rmd-proprio-gain` and
  `--head-motor-preset` (SMD/RMD as a separate type) are tunable.

Closed-loop sweeps at depth 3 (`target/qa/celegans-tune2..6/`, 10 s each,
frozen anatomy) changed head gain 0.9-4, head extent 0.15-0.25, RMD
proprioception 0-2, bursting (CH) head cells, B-type gain 0.6-2.5,
junction gain 12-30, gap unit 0-0.05 and transducer recovery 1.5-5 s. With
mechanics v3 every joint wriggles (23/23) and curvature rises to kL ~1.5, but
head-swing regularity (autocorrelation peak) stays 0.13-0.21, propagation
direction is random (forward fraction ~0.5) and D/V correlation stays +0.06 to
+0.14. Head parameters barely matter because SMD makes one synapse per head
muscle in this connectome; head rows are mostly driven by IL1, RME, SMB and URA.
B-type positive feedback holds a ventral bias (VB 45 Hz vs DB 15 Hz) rather
than passing bends back.

**Depth study v2** (`target/qa/celegans-depth-study-v2/`: `REPORT.md`,
`comparison.png`, `study.json`, one MP4 per depth; network sha256
`4fed438b...`, frozen anatomy). Closed loop: 30 s per depth, lock-step,
15,000/15,000 frames applied; wall-clock 6-12x simulated time. Without the
encoder's random floor the closed loop is deterministic, so depths 2-5 are
identical (as they are open loop).

| Depth | Curvature kL (worm 3-9) | Undulation Hz (worm 0.25-0.7) | D/V muscle corr. (worm < -0.2) | Muscle activation | Net move in 30 s | Reversals/min |
|---|---|---|---|---|---|---|
| 0 | 0.27 | 0.57 | +0.83 | 0.70 | 0.020 L | 0 |
| 1 | 1.97 | 0.37 | +0.04 | 0.21 | 0.524 L | 42 |
| 2-5 | 1.48 | 0.97 | +0.03 | 0.15 | 0.152 L | 10 |
| passive | - | - | - | 0 | 0.007 L | - |

Open loop (140 runs): no non-finite state; depths 4/5 bit-identical to 3;
depth 0 hypersynchronous (624 frames with > 30 % of cells active); depths
1-3 asynchronous at 9, 6 and 6 Hz with D/V output correlation +0.03 to +0.09
(was +0.4). STP (-29 Hz) and the NMDA gate (-30 Hz) remain the stabilisers,
active dendrites add +30 Hz, and 28/128 permutations burst. Because STP is a
depth >= 1 mechanism, exteroceptor adaptation exists only from depth 1, where
sensory responses become onset transients (e.g. ASH 8 Hz under touch vs 6 Hz
baseline; depth 0: 315 vs 25 Hz); the 1.5 s recovery may be too strong.

**Result.** Depth 1 is now the closest to the animal: the worm bends, holds
and shifts a curved posture and travels 0.5 body lengths in 30 s (passive
0.007), with undulation frequency in range. It is still not crawling: the
kymograph shows a standing, ventrally biased bend with head flicker rather
than a head-to-tail wave, D/V activity is uncorrelated instead of antiphase,
speed is ~10x below a worm's and reversals are ~10x too frequent. The body can
crawl when commanded correctly (programmed wave: 0.16 L/s), so the remaining
gap is neural: no rhythm generator emerges from this connectome with spiking
Izhikevich surrogates. Open options: graded (non-spiking) motor neurons and
muscles (C. elegans motor neurons are mostly graded), bistable/plateau B-type
units as in Boyle, Berri & Cohen (2012), or measured neuromuscular weights for
the head circuit.

### C. elegans graded motor neurons and muscles (2026-11-21)

C. elegans motor neurons signal with graded potentials and tonic transmitter
release, and body-wall muscle force follows a graded Ca2+ level. Both can now
be modelled as graded cells, as an option next to the Izhikevich spiking
surrogates (which remain the default).

**Runtime** (`src/neuron_kernels.rs`, `src/runner.rs`, `src/config.rs`). A
per-type `graded_*` block in `AarnnBioParams` (off by default) replaces a
cell's spike transition with

`tau dV/dt = -(V - V_rest) + P(V) + g (E - V)`, where `g (E - V_rest) = R I`.

- The cell receives exactly the total current a spiking cell would (chemical,
  electrical, sensory). It acts as a conductance with reversal 0 mV (net
  excitation) or -80 mV (net inhibition): near rest it is the current `R I`,
  but depolarisation saturates. A first current-source version reached
  +85 mV and, through gap junctions, drove AVA/AVB to 53-73 Hz.
- `P(V)` is an optional regenerative plateau (bistable when larger than 4x
  its slope), used for B-type and RMD/SMD plateau potentials.
- Integration: exponential midpoint in sub-steps of at most 0.25 ms. It is
  exact for the linear membrane (within 1e-9 mV of the analytic step response
  at dt 0.1-2 ms) and within 0.25 mV of a 0.001 ms RK4 reference through
  bistable switching. (A frozen-plateau first version erred by 1.03 mV.)
- Release is `(logistic((V - V_half)/k) - base) / (1 - base)`: exactly zero
  at rest. Rest is -70 mV, the rest of the b = 0.2 Izhikevich presets, so gap
  junctions between graded and spiking cells carry no tonic current.
- Graded synapses: a cell at release `r` adds `w r f_max dt` of
  spike-equivalent drive into the same depth-gated synaptic stage as spikes
  (instantaneous charge at depth 0, AMPA/NMDA/GABA kernels at depth >= 1). It
  therefore delivers the mean charge of a spiking cell firing at `r f_max`.
  Graded transmission has no STP, release probability or axonal delay.
- Graded cells never spike and carry no adaptive threshold, refractoriness or
  homeostasis. They always use the CPU reference transition; the device
  transition is disabled for graded networks.

**Graded muscles and the body.** A graded output cell sends its activation as
a pulse-density code: a first-order sigma-delta at `graded_release_max_hz`.
At 500 Hz there is at most one event per 2 ms body frame, so the fraction of
frames carrying an event equals the activation.

- When every output cell is graded, the node adds `"output_code":"pulse_density"`
  to its handshake reply, and the bridge passes it through unchanged.
- The body (`aarnn-embodiment` `OutputCode`) then decodes the train linearly
  with the same 55 ms excitation-contraction filter. Twitch summation would
  compress the code: a steady 0.2 reads about 0.67.
- Every trace record stores the negotiated `output_code`.

**Importer options.**

| Option | Effect |
|---|---|
| `--motor-model graded` | Every motor neuron becomes graded except AVL and DVB, which fire action potentials (Jiang et al. 2022). |
| `--muscle-model graded` | Muscles become graded. |
| `--b-plateau-mv`, `--head-plateau-mv` | Plateau strength of B-type and of SMD/RMD cells. |
| `--graded-tau/-resistance/-v-half/-release-hz`, `--muscle-tau`, `--muscle-resistance` | Cell parameters. |
| `--b-proprioception stretch` | Boyle, Berri & Cohen (2012) B-type mapping: DB (VB) integrates stretch of its own side locally and posteriorly over `--b-field` (0.5) of the body, with gain normalised per neuron. The default remains `curvature`: same-side curvature just anterior (Wen et al. 2012). |

None of these options changes the wiring (tested).

**Tools.**

- `scripts/qa/celegans_depth_study.py` gains `--network`, `--config`, `--set`,
  `--passive` and `--cell-models-from`. The last grafts another build's cell
  models onto a frozen anatomy and refuses wiring changes. It accepts
  value-only changes of sensory gains and gap conductances on the same
  pattern: a frozen snapshot holds one morphology route per sensory entry and
  silently has none for a new one (161 weights, 153 routes).
- `celegans_depth_probe --series` writes the 20 ms activity and voltage of
  every cell; graded cells report their spike-equivalent rate.

**Calibration** (open-loop replays of recorded closed-loop sensor traces, then
closed-loop sweeps; `target/qa/celegans-graded-sweep1..5/`).

- *Saturation.* Every network gain was calibrated for spiking cells, so graded
  cells saturate. Inferred drive was 28-206 mV/ms to DB and 52-170 mV/ms to
  SMD, against roughly 5 mV/ms for half release. Stretch alone gave
  164 mV/ms per unit of stretch.
- *Coil.* At 50 Hz-equivalent release every graded stage saturated. The
  network then fell into its most strongly wired state: DD receives 32
  VB and 30 VA synapses per cell, against 10 DB and 9 DA per VD. VB/VA drove
  DD, DD silenced the dorsal muscles, and the body held a ventral coil with
  a co-contracted head.
- *Latch.* A 30 mV plateau makes B-type cells switch on at 1.3 mV/ms and off
  only below -0.3 mV/ms, so they latch on. With 22 mV both thresholds are
  positive, like the 0.25/0.75 thresholds of Boyle, Berri & Cohen.
- *Calibrated recipe.* Release 20 Hz-equivalent (now the importer default),
  NMJ gain 30, B/SMD/RMD stretch gains 0.05, B plateau 22 mV, stretch mapping
  with RMD stretch input. In replay, motor neurons then work mid-range (DB at
  15 of 20 Hz-equivalent) and about 16 % of muscle samples saturate.
- *Unchanged by cell model.* Head and B-type parameters did not create
  dorsal/ventral alternation in any of 27 graded closed-loop tuning runs
  (15 s each).

**Depth study: graded vs spiking on the same anatomy.** Both use the frozen
anatomy `network_celegans_frozen_bbc.json` (RMD stretch input, stretch
mapping), 30 s per depth in lock-step, 750-frame videos at simulation speed.
Evidence: `target/qa/celegans-depth-study-graded/` and
`target/qa/celegans-depth-study-bbc-spiking/` (`REPORT.md`, `comparison.png`,
MP4 per depth). The graded study also reran the open-loop 140-run factorial.

| Depth | Cells | Curvature kL (worm 3-9) | Undulation Hz | D/V muscle corr. (worm < -0.2) | Net move in 30 s | Reversals/min |
|---|---|---|---|---|---|---|
| 0 | spiking | 0.17 | 0.10 | +0.81 | 0.024 L | 0 |
| 0 | graded | 0.50 | 0.20 | +0.60 | 0.019 L | 0 |
| 1 | spiking | 1.75 | 0.23 | +0.12 | 0.137 L | 36 |
| 1 | graded | 2.87 | 0.07 | +0.07 | 0.322 L | 54 |
| 2-5 | spiking | 1.36 | 0.23 | +0.09 | 0.221 L | 20 |
| 2-5 | graded | 2.86 | 0.70 | +0.08 | 0.454 L | 60 |
| - | passive | - | - | - | 0.007 L | - |

Open loop (graded): no non-finite state; depths 4/5 are bit-identical to 3;
depth 0 still synchronises (192 frames). At depth 3, STP (-24 Hz hidden,
-131 Hz muscle events) and the NMDA gate (-23 Hz) stabilise, and active
dendrites add +25 Hz.

**Result.** Graded motor neurons and muscles work as specified and are
numerically verified. On the identical body and wiring they roughly double
curvature and travel compared with spiking cells. They do not make the worm
crawl: dorsal and ventral muscles never alternate (correlation stays
positive), no head-to-tail wave forms, and the net-zero drift reads as 50-60
reversals per minute. The replay diagnostics locate the remaining gap in the
circuit's operating point rather than in cell dynamics:

- head rows co-contract (driven mostly by IL1, RME and RMD on both sides);
- D-type drive is three times stronger on the dorsal-inhibiting side;
- AVA and AVB are co-active, so forward and backward motor programmes
  compete.

**Also fixed.**

- Lock-step backstop: the adapter's fixed 30 s backstop aborted a contended
  depth-0 run. It now extends to the configured
  `NM_AER_REFERENCE_DATA_TIMEOUT_MS` plus 5 s.
- Clippy with warnings denied failed on code from the previous update
  (`Taps::active`, the C. elegans sensor-mount type); both are fixed.
- This section's predecessor ran straight into the launch notes below without
  a paragraph break; the notes now have their own heading.

### C. elegans graded command interneurons and forward state (2026-11-21)

Follow-up to the graded study: AVA and AVB are graded too, and a forward
state (AVB on, AVA off) is imposed.

**Runtime.** `AarnnBioParams::tonic_current` (mV/ms, default 0) is a constant
input current added to a cell every step, after all synaptic, electrical and
dendritic terms. It works for graded and spiking cells and represents a
behavioural-state or descending drive. Boyle, Berri & Cohen (2012) drive
B-class neurons with exactly such a constant AVB current. A test checks the
analytic graded steady state under the bias and that a biased spiking cell
fires.

**Importer.** `--graded-classes AVA,AVB` makes any listed class graded.
`--tonic CLASS=VALUE` (repeatable) sets a class's tonic current. A class
named in either option gets its own type (e.g.
`Interneuron:acetylcholine:graded:AVB`). Per-type parameters are now derived
from the cell class, so the B-type and head plateau types are unaffected
(tested).

**Calibration** (open-loop replays of a closed-loop trace, depth 3).

- With AVA/AVB graded and no bias, AVB already leads AVA (89 % of 20 ms
  windows). AVA nevertheless sits at about 40 % release: an inferred net
  drive of 4.7 mV/ms, against 7 for AVB.
- A bias injected into AVA is shunted by its 230 gap-junction contacts
  (mostly VA, DA): -3 mV/ms lowered its net drive by only 0.75. AVA is off
  (release about 0.1) at -15 mV/ms.
- The A-type motor neurons fall only from about 4.5 to 3.5 Hz-equivalent,
  because the spiking AVD/AVE still drive them.
- AVB couples to every B-type cell through 36-46 gap contacts each, and its
  fluctuations are a common-mode drive. DB and VB pool activity correlates at
  +0.73 although their stretch inputs anti-correlate (-0.48). Pinning AVB
  with a large bias (40 mV/ms; voltage s.d. 5.1 -> 0.9 mV) halves AVB-B
  correlation but leaves DB-VB at +0.84 with the 22 mV plateau (+0.25 to
  +0.33 without it). Other network inputs dominate the B-type cells.
- Twelve closed-loop variants (`target/qa/celegans-graded-sweep6..7/`, 15 s,
  depth 3) imposed the forward state, biased AVD/AVE down as well, pinned
  AVB, and varied stretch gain 0.05-1.0 and the plateau. Dorsal/ventral
  correlation stayed between +0.08 and +0.15 and no forward wave formed.

**Final study** (`target/qa/celegans-depth-study-forward/`: graded motor
neurons and muscles plus graded AVA/AVB with AVB +2 and AVA -15 mV/ms; same
frozen anatomy as the graded and spiking studies; 30 s per depth, videos).

| Depth | Curvature kL | D/V muscle corr. | Net move in 30 s | Open-loop AVA / AVB (Hz-equivalent) |
|---|---|---|---|---|
| 0 | 0.64 | +0.27 | 0.029 L | 3.7 / 4.9 |
| 1 | 2.63 | +0.11 | 0.315 L | 1.0 / 8.7 |
| 2-5 | 2.61 | +0.11 | 0.313 L | 1.1 / 8.9 |

Replaying the depth-3 closed-loop input shows how clean the forward state
is:

- *Forward-state network:* AVA 1.9 and AVB 13.5 Hz-equivalent, with AVB
  above AVA in 99 % of 20 ms windows.
- *Previous graded network* (spiking AVA/AVB): AVA 10.4 and AVB 19.3 Hz,
  with AVB above AVA in only 33 % of windows.

Open loop: no non-finite state; depths 4/5 bit-identical to 3; depth 0
synchronises less than before (53 frames, was 192).

**Result.** The forward command state is clean, but it does not produce
crawling. Kinematics match the graded study (kL 2.6 vs 2.9), dorsal/ventral
muscle correlation stays positive, and net travel is a little lower (0.31 vs
0.45 L in 30 s). Behaviour is limited downstream of the command layer: DB
and VB co-vary (+0.70 in closed-loop replay) because network inputs other
than stretch dominate them.

*Metric note:* `reversals_per_min` counts sign changes of the centroid's
axial velocity (hysteresis 0.02 L/s). When net speed is near zero it counts
jitter: these runs score 56-62 per minute while the brain holds the forward
state throughout.

### Embodied locomotion across bodies: designed AARNN circuits (2026-11-22)

Goal: every network/robot pair moves in its habitat the way its closest
animal would, with the lowest latency, using one set of algorithms
specialised per body. Imported connectomes that cannot be placed (fly
BANC/FAFB, zebrafish) and the untrained archetype robots received
*designed* AARNN circuits; the worm keeps its connectome
(`network_celegans_frozen_bbc5.json`, sections above).

**Common algorithm, in layers** (`scripts/aarnn_circuit.py`):

1. *Cell*: graded (non-spiking) AARNN cells with conductance input, optional
   bistable plateau, tonic current; motor outputs send a pulse-density code
   (`output_code = pulse_density`) that every body decodes linearly.
2. *Spinal*: the half-centre - two bistable cells with reciprocal inhibition -
   switched either by proprioception (worm B-type stretch, hexapod coxa
   extremes) or by its own slow adaptation (fish spinal segments, fly and
   quadruped legs: `add_leg_oscillators`); neighbour coupling (swing inhibits
   neighbouring swing, excites neighbouring stance) gives tripods for six
   legs and trot pairs for four. Lift must lead protraction (swing
   adaptation cuts the lift, protraction goes through a delay cell): this
   alone made the fly walk five times faster. Joints driven by torque use a
   stretch reflex (`add_stretch_reflex`) whose set point the oscillator moves.
3. *Hindbrain* (`add_hindbrain`): `MLR` tonic command (speed); bistable
   winner-take-all `RS_l`/`RS_r` turn decision whose hysteresis carries a turn
   through; `ARTR`, a slow half-centre (larval zebrafish anterior
   rhombencephalic turning region, Dunn et al. 2016) that alternates the
   preferred side every ~10-20 s (exploratory arcs, head-on tie-breaks);
   obstacle rays excite the ipsilateral `RS` (turn away); wheeled bases add a
   stall cell and a self-terminating reversal (the worm's AVA backward state).

Design utilities `steady_release` and `bistable_band` mirror the runtime's
graded equations (22 mV plateau: bistable between 0.46 and 1.57 mV/ms).
Two runtime facts shape every design:

* *Inherited cell nonlinearities.* Designed circuits take their base biology
  from `network.json`, which enables active dendrites (a calcium plateau that
  multiplies positive net synaptic current by up to ~1.4 with 120-350 ms
  state), adaptive thresholds and network-wide STP. Measured as "excitation
  delivers 1.373x nominal", this was the dendritic gain, not a kernel
  constant. `Circuit(exact_cells=True)` switches them off so cells follow the
  design equations exactly (verified: 5.0, 19.9, 5.0 mV/ms for designs of 5,
  20, 5). The fish, fly, hexapod and archetype explorer were tuned before
  this option and keep the inherited cells (with `exact=True` weights where
  they cancel offsets).
* *Pulse-coded senses.* A sensor value arrives as a ~250 Hz pulse train. A
  cell that cancels its 0.5 baseline with a large inhibitory tonic sees the
  net current flip sign every few ms; inhibitory conductance is ~7x more
  effective per mV/ms, so it reports a duty cycle (a balance pair summed to
  exactly 1 and ignored tilt). Analogue senses therefore go through
  excitation-only relays whose release threshold sits at the operating point
  (`graded_release_v_half_mv`, steep slope), calibrated in the runtime by
  `measure_sensor_relay` (relay at 0.48 upright, 2.7 release per sensor unit).

AARNN depth also changes designed circuits, so each builder states its
validated depth (fish and hexapod 3, fly, archetypes and balance 0).

**Results** (MuJoCo, lock-step, 30 s unless noted; `scripts/qa/probe_embodiment_motion.py`;
travel speed = root path averaged over one locomotor cycle; latency = median
brain reply on a loaded 16-core host):

| Body | Circuit | Behaviour | Speed | Rhythm | Exploration | Latency |
|---|---|---|---|---|---|---|
| C. elegans | connectome bbc5 + graded motor | head-first crawl, D/V antiphase -0.44 | +0.19 L/s axial, 75 % forward time | 1.27 Hz (fast vs 0.25-0.7) | 9 cells, hull 5.5 L^2 | ~2 ms/frame |
| Zebrafish v2 | `build_zebrafish_swim_network_json.py`, 55 cells | head-to-tail wave 0.11 cycle/segment, wall avoidance, ARTR arcs | 0.64 L/s travel | 4.5 Hz | 20 cells, hull 13.7 L^2 | 3.3 ms |
| Drosophila BANC | `build_drosophila_walk_network_json.py`, 49 cells | tripod (lf,rm,lr / rf,lm,rr within 9 deg), stance duty 0.67 | 0.23 L/s travel (15 s) | 3.7 Hz | 2.5 L net in 15 s | 4.1 ms |
| Hexapod | `build_hexapod_reflex_network_json.py` (depth 3) | Cruse-rule stepping | 0.045 L/s | 1.9 Hz | - | 2-3 ms |
| Keenon T8 | archetype explorer | crosses room, turns at 0.3-0.8 m | 1.0-1.2 L/s path | - | 41-44 cells, hull 59-76 L^2 | 1.9-2.6 ms |
| Keenon T9/T11/C30/C40 | archetype explorer | room exploration | 0.8-1.5 L/s path | - | 26-52 cells | 2-4 ms |
| Keenon T10, S100, W3 | archetype explorer | wedge under a table top (see limits) | 0.27-0.44 L/s path | - | 10-18 cells | 2.0-2.6 ms |
| Galbot G1 (omni base) | archetype explorer + posture | drives, 27 joints held | 0.27-0.41 L/s path | - | 7 cells | 2.9 ms |
| UGOT engineer/transform | archetype explorer | barely move: 0.1 N m wheels against 0.05 N m s/rad damping | 0.05 L/s path | - | 2 cells | 2.5-2.6 ms |
| Cofe, Xbot, Botinkit (fixed bases) | posture reflexes | arms hold their pose | - | - | - | |
| UGOT quadruped (OEM legs) | `build_legged_explorer_network_json.py`, 105 cells | trot, run-and-turn exploration, backs off and turns away from the rib mat, never falls (40 s) | 0.23 L/s travel | 1.9 Hz | 8 cells, hull 2.4 L^2 | 2.8-6.8 ms |
| UGOT spider (OEM legs) | same | trot, run-and-turn (slow pivot), never falls | 0.10 L/s travel | 1.9 Hz | 5 cells, hull 0.35 L^2 | 2.8 ms |
| UGOT balance | `build_balance_network_json.py`, 20 cells | balances and explores, never falls (OEM wheels: rate gain 4.5, max tilt 7.6 deg in 30 s) | ~0.08 m/s path (was ~0.26 before the OEM wheels) | - | 9 cells (was 29, hull 33 L^2) | 3.5 ms |
| UGOT wheel-leg (OEM legs) | same, 36 cells | balances on servo-held legs, never falls | 0.16 m net | - | 3 cells | 2.7 ms |
| NAO (OEM) stand | `build_nao_stand_network_json.py`, 40 output cells | holds StandInit 30 s, lean 6.33 +- 0.06 deg | - | - | - | 2.6 ms |
| NAO (OEM) walk | `build_nao_walk_network_json.py`, 31 cells | slow walk with lateral weight shift and sonar avoidance: turns away from walls, follows them, backs off when near; never falls (3 x 200 s from 3 approach angles) | ~2 cm/s | 0.5 Hz | closest torso-to-wall 0.20-0.24 m (no avoidance: walks into the wall) | 2.6-3.7 ms |
| Crow (flight v2) | `build_crow_network_json.py`, 71 cells | flapping flight from an in-flight release: 10.6 s, 97 m aloft at 2.1-5.2 m, ends at the tree line (see subsection) | 9.9 m/s airspeed | 4.3 Hz | banked ARTR arcs | 4.4 ms |

**Self-balancing UGOT** (`scripts/build_balance_network_json.py`, exact
cells): an inverted pendulum held by vestibulospinal reflexes. The IMU's
gravity direction (new `body_gravity` channel for the balancing archetypes,
posture-scale ranges: gravity +-0.5 g, gyro +-8 rad/s) plays the otoliths and
the gyro the canals; excitation-only relays feed a push-pull pair B+/B-
(tilt, tilt rate, wheel speed) driving both wheels; the MLR leans the set point
forward and back-EMF wheel damping turns the lean into a cruising speed; the
shared hindbrain steers by wheel differential. Archetype wheels now carry
back-EMF damping (stall torque / 25 rad/s no-load speed; the default .05 held
UGOT wheels to 1-4 rad/s, too slow to drive under a falling body). Lagged-PD
feasibility first: kp 10, kd 0.15-0.3 (ctrl units per rad, rad/s) balances
through 30-40 ms of lag; kd 0 falls. Result, 30 s: never falls, explores 29
cells (hull 33 L^2, ~7.8 m of path), recovers from bumping a 3 cm rib mat
(38 deg back to upright), median latency 1.9-2.1 ms; bit-identical on the
sharding agent's new runtime binaries. The wheel-leg variant, on its OEM leg
servos and ripple-safe servo reflexes (below), now balances for 30 s too.
*Re-tune for the OEM wheels (2026-11-23):* the UGOT OEM back-EMF (2 kgf cm at
360 rpm, 0.0052 N m s/rad instead of 0.0078 at 25 rad/s) left the pitch loop
underdamped: the robot fell within 2 s, with or without proximity steering.
The pitch-rate gain is now 4.5 (was 3): 30 s, max tilt 7.6 deg, no fall, but
it explores less (2.3 m path, 9 cells). The wheel-leg variant still falls at
the new gain and needs its own re-tune.

**UGOT legs and NAO from OEM specifications.**

* UGOT (UBTECH): smart servos 13 kgf cm (1.27 N m) at >= 60 rpm, gear motors
  2 kgf cm (0.196 N m) at >= 360 rpm, back-EMF damping = stall torque /
  no-load speed, 2-servo legs (hip and knee pitch for the quadruped, hip yaw
  and knee for the spider), 2.1 kg. Four network-side faults hid the gait:
  1. *Escape oscillators need 30 mV plateaus in exact cells.* Measured
     open-loop: at 22 mV the leg half-centres sat co-active at 0.6
     (0 Hz); 26-35 mV give a trot at 1.4-2.4 Hz (diagonals +0.98,
     neighbours -0.7 to -0.9) set by the adaptation constants and the MLR
     drive. `add_leg_oscillators` now defaults to 30 mV with `exact_cells`;
     the slow ARTR half-centre likewise stalled with both sides off.
  2. *Servo reflexes must smooth the pulse ripple before the gain.* With
     10 ms relays, kp x ripple was half-wave rectified by Q+/Q-: both sat
     at ~0.4 at zero error (co-contraction), with a 0.08 rad offset and
     ~2 N m/rad. 30 ms relays (`add_servo_reflex(relay_tau_ms=30)`) remove
     the offset and give ~6.5 N m/rad. The wheel-leg's legs moved from
     `add_stretch_reflex` to this servo.
  3. *Posture.* The dog stands hip-forward/knee-bent (-0.4/0.8 rad), so knee
     flexion lifts the foot to first order.
  4. *Turning a skid-steer leg pair.* Shortening the inner strides pivoted
     at ~5 deg/s. Now the inner legs step backward (`Gr` relays, an AND of
     leg phase and the turn command); spontaneous turn bouts come from a
     bout clock whose side the slow ARTR picks (`add_turn_bouts`, larval
     zebrafish run-and-turn), each opened by a 0.3 s back-off of all legs
     (C. elegans reversal / fly moonwalker).
  The terrain's traction mat (eight 3.9 cm ribs at 6.5 cm pitch) is
  impassable for 12 cm legs: a programmed trot stops at the first rib too,
  and the torso-height rays cannot see it. The walker backs off and turns
  away from it. The UGOT terrain rims rose to 0.42 m: on the OEM legs the
  rays (0.1 m above the body origin) passed over the 0.234 m catalogue rims.
* NAO v2 (Aldebaran H25 V6): link lengths, per-link masses (5.305 kg), joint
  axes (HipYawPitch about the 45 deg Y-Z axis, coupled left/right by an
  equality, as the single motor is) and ranges; torque = motor stall x
  reduction (leg pitch 8.5 N m, rolls and yaw-pitch 13.1 N m), speed =
  no-load / reduction (6.96 / 4.53 rad/s), back-EMF damping, rotor armature
  (estimate), position kp = stall torque per 0.1 rad. The OEM zero pose is
  the built pose (ref 0), so joint angles read as on the robot. The sole
  (160 x 88 mm) is an engineering estimate. Cameras sit at their OEM head
  positions. Two actuation details made a graded brain topple it, fixed in
  `aarnn_embodiment::actuation` as opt-in record fields (only NAO sets them):
  `wake` (on arming, targets blend from the rest pose over 1 s and the
  decoder starts at the rest-pose density - a silent graded brain otherwise
  commands every joint to its lower limit for ~50 ms, as NAO itself
  interpolates on wake-up) and `decode_tau_s` (200 ms: at the generic 20 ms
  the pulse-density ripple, +-5 % of a joint's range, topples the biped in
  ~4 s; at 100 ms the feet stick-slip backward 9 mm/s). The stand circuit's
  vestibular ankle strategy is built but off: every gain tried did worse
  than posture alone (accelerometer dominated by contact vibration; relay
  warm-up offsets).

* NAO walk (`scripts/build_nao_walk_network_json.py`, 9 cells): the
  StandInit posture plus a slow walk command (3 s rise, so the body settles
  after the wake-up), one half-centre pair alternating "body over the left
  foot, right leg swinging" and its mirror at 0.5 Hz (1.9 s adaptation, the
  oscillator's start-up lead balanced so both phases last 0.99 s), lateral
  weight shift (hip roll -0.3 / ankle roll +0.3 rad on both legs), swing
  flexion (0.25 rad, knee 2x) and a stride (0.1 rad, trim 0.6) through delay
  cells (300 ms) that lag the phase by about a quarter cycle - a programmed
  square-phase gait advanced 0.10 m in 20 s without that lag and 0.55 m with
  it. Feasibility first: a programmed sinusoidal gait through the same 200 ms
  decode walked 0.97 m in 40 s straight; replaying the network's decoded
  targets reproduced the network's curve, so the heading drift is in the
  target pattern, not the adapter.
* NAO sonar avoidance (same builder, `add_sonar_avoidance`): the chest
  sonars (v1 aimed both straight ahead - the capitalised names
  `Sonar/Left`/`Sonar/Right` matched no side suffix; now 20 deg to each
  side, one ray each) reach excitation-only relays at 0.9 m; proximity cells
  drive the shared hindbrain's bistable turn cells, and a turn shortens the
  stride on the side to turn towards (per-leg delay cells; inner -12,
  outer -4 mV/ms). Below 0.4 m a back-off bout (latching BACK cell with its
  own adaptation, ~4 s) drives each stride cell from the opposite
  half-centre through AND cells, which reverses the stride; a slow REV cell
  (600 ms) carries the gating so the stride passes through zero. 200 s,
  three approach angles: the walker turns away and follows the wall at a
  sonar range of ~0.4 m, backs off and turns in corners, closest
  torso-to-wall 0.20-0.24 m, no falls; without avoidance it walks into the
  wall at 70 s. A stronger backward stride (AND drive 41) left the corner
  faster but fell in the head-on approach; inner-only or stronger stride
  steering and hip yaw-pitch steering (programmed: 39 deg/28 s, but
  non-monotonic in amplitude and stride) were worse or fell.

**Body and habitat fixes found on the way** (all regenerated; IO digests
change only for the fish):

* Zebrafish mechanics v2: the legacy damping .02 / armature 1e-4 against
  0.2 mN m tail servos allowed ~0.01 rad/s (no tail wave); disc-shaped
  segments swam backwards. Now a tapered, laterally compressed neutral-density
  trunk, vertical caudal fin hinged at the peduncle, striped skin, and an
  engine-independent water model `scene.hydrodynamics`
  (`resistive-ellipsoid-v1`: per-ellipsoid anisotropic quadratic drag
  Cd = 0.1/1/1 along body x/y/z, rotational drag, neutral buoyancy 0.5 mm
  dorsal to each centre of mass - the swim bladder; without it the trunk
  pitches into a dive within 5 s). Implemented once in
  `aarnn_embodiment::force::ellipsoid_drag` for every engine. A programmed
  6 Hz, 1 L wave swims ~0.6 L/s with the native servo limits.
* Drosophila mechanics v2: same frozen-joint cause; damping 2e-4, armature
  1e-6, servo kp .03 (policy text updated; native torque limits kept).
* Wheeled archetypes drive-v2: four fixed high-friction wheels could not
  skid-steer (half torque yawed T8 0.003 rad in 0.6 s); the front pair now
  rolls like casters. Forward/turn wheel patterns are calibrated per robot
  (`scripts/qa/calibrate_wheels.py`; omni and mecanum bases differ).
* Range rays started inside the robot (fish jaw, 18 of 24 fly rays on its
  legs, galbot chassis): `Physics::ray_beyond_self` steps over the robot's
  own tree for legacy and archetype rays.
* Habitats: the fish tank's water stood above its walls and the fish spawned
  above them; every non-worm habitat had a low front rim, and room walls were
  shifted with the furniture leaving 1.5 m corner gaps - robots drove out.
  Rims are now full-height (glass front) and never shifted.
* Archetype robots decode a graded brain's positive/negative channel pairs
  linearly (`actuation.rs`, test `graded_antagonist_rate_decodes_signed_density`).

**Cross-engine parity and distribution** (see the agent reports in this
session): Webots runs the hexapod reflex circuit on a new managed route
(`run_sim.sh --sim webots --control aarnn`) at parity with MuJoCo (0.334 vs
0.315 m net in 30 s, 1.63 vs 1.90 Hz). Unity, Unreal, Minecraft and WebGL
decode `pulse_density` like the Rust crate; their graph/Webots exports now
carry each joint's reference angle (`qpos0`; the Webots export had femur and
tibia offset by -0.8/-1.2 rad); Unreal (UE 5.8) builds. The WebGL/Minecraft
physics service gains `NM_PHYSICS_SERVICE_LOCKSTEP=1` (at real-time pace 37 %
of frames were skipped and the hexapod stepped at 0.73 Hz). Distributed
execution now drives the body from the shards themselves: with
`NM_CLUSTER_NODES > 1` and one brain, every population (each hidden layer and
the outputs) is split into contiguous cell ranges (`shard_cell_range`); the
IPC owner is shard 0 and coordinates lock-step (STEP with the 2 ms sensory
frame, SLICE/MERGED exchange of spikes and membrane voltages at each
population's commit, DONE before the reply; `src/neuron_shard.rs`), so
one-layer circuits and connectomes split too and no node computes the whole
network. Closed loop, 10 s, lock-step: hexapod reflex and zebrafish swim are
bit-identical at 1, 2 and 3 nodes (same 41187 / 26034 output spikes, same
trajectories), at +0.1-0.8 ms per frame for these 24- and 55-cell circuits.
In-process tests require per-step bit-identity for graded, tonic, gap,
morphology-delay, pulse-density and stochastic cases and for the shipped
hexapod, fish and worm networks. `NM_BODY_SHARDING=0` restores the previous
behaviour. Shards of frozen, non-morphology networks now store only their own
weight rows (1/N of the weights: 8,000 one-layer cells hold 777 MB on one
node, 130 MB per shard on 4; INIT ships each follower its own rows in binary)
and exchange only what other shards read (fired cells as index/value pairs,
voltages only for gap-coupled or cross-shard graded cells); hexapod and
zebrafish closed loops stay bit-identical at 1, 2 and 3 nodes on this build.
Step time improves only ~2x at 4 shards (every shard still runs transition,
STP and history for all cells); per-cell state, topology and the gap graph
are replicated, and morphology/growth/plasticity/OpenCL networks (the worm,
BANC) still replicate the whole network. Lib tests 603 passed.

Isaac Sim 6.2.0 (pip, Kit 110.4, CPU PhysX on the RTX 2080; 25 GB in
`target/isaacsim-venv`; the NVIDIA Omniverse EULA is accepted per run via
`OMNI_KIT_ACCEPT_EULA=YES`) runs on the same managed, lock-stepped route
(`run_sim.sh --sim isaacsim`), with the body logic in the Rust FFI library
(new `nm_reference_apply`/`nm_reference_submit`/`nm_reference_hydro`) and
sensors ported to Python (verified bit-identical against MuJoCo through a
MuJoCo twin owner). Hexapod reflex circuit, 30 s: 0.307 m vs 0.315 m net,
1.7 vs 1.9 Hz, latency 2.71 vs 2.58 ms median; worm 10 s: 0.0324 vs 0.0337 m.
The USD generator still has to emit joint limits relative to `ref`,
armature/damping, friction materials, collision filters and equality
constraints (the runner patches these in its session layer; the bases with
constraints - cofe, xbot, keenon, galbot - are refused). See
`sim/isaacsim/README.md`.

**Limits.** Archetype bases carry one ray ring at chassis height and a gyro:
a table top above the ring is invisible and wheels that slip in place look
like motion, so T10/S100/W3 can wedge (needs a second ray ring, bumper or
odometry - an IO contract change). The UGOT walkers cannot cross the
terrain's 3.9 cm rib mat and see it only by bumping into it; the spider
pivots slowly (0.10 L/s). The NAO walks slowly (~2 cm/s at 0.5 Hz: the
200 ms decode needed against pulse ripple limits the gait's speed); its
heading is sensitive to the stride trim (0.5 curves -60 deg, 0.6 straight,
0.7 +38 deg in 40 s) and it has no obstacle steering yet; the vestibular
reflex is off. Obstacles below the sonar beams (the 0.22 m ball, table
legs, the 0.27 m table top) are not sensed: the walker can stay pressed
against the ball. The NAO foot bumpers are unusable as obstacle senses: the
directional contact test fires on ordinary ground contact (36-68 % of the
time while walking); real front bumpers need a test on the contact normal in
every adapter. Heading control by stride asymmetry is slow (~1 deg/s) and
corners take tens of seconds. NAO in the Webots native route stops on the FSR channels
(`legacy-contact-force` is MuJoCo-only; the preserved Webots NAO route uses
Webots' own Aldebaran model). Unity and Unreal decode NAO like MuJoCo now
(decode tau and wake in `NmGradedJoints`/`FGradedJoints`, offline vectors
match Rust to 1e-12; OEM ranges, drives and links from
`scripts/generate_nao_engine_tables.py`), but neither was run: no Unity
Editor on this host, and the Unreal graph path lets every robot fall
through the floor (hexapod too); the graph also lacks rotor armature, which
the explicit drive torques on 2 g phalanges need. The crow's and the fly's
flights have their own subsections below. Fish webots/Isaac
water and worm agar drag are not yet ported outside MuJoCo.

Reproduce (MuJoCo SDK 3.17.0, runtime built as above):

    python3 scripts/build_zebrafish_swim_network_json.py
    python3 scripts/build_drosophila_walk_network_json.py
    target/robot-mujoco-venv/bin/python scripts/build_archetype_explorer_network_json.py keenon_t8
    python3 scripts/build_legged_explorer_network_json.py ugot_quadruped   # and ugot_spider
    python3 scripts/build_balance_network_json.py ugot_wheelleg
    python3 scripts/build_nao_stand_network_json.py
    python3 scripts/build_nao_walk_network_json.py
    # then run_sim.sh --sim mujoco --robots PROFILE=1 --lockstep --headless \
    #   --network-map PROFILE_0=network_... --config-map PROFILE_0=webots_world/configs/config_...
    python3 scripts/qa/probe_embodiment_motion.py TRACE.jsonl
    MUJOCO_GL=egl target/robot-mujoco-venv/bin/python scripts/qa/render_trace_video.py TRACE.jsonl OUT.mp4 [--fixed X,Y,Z]

#### Crow flight (`crow_corvus_corone`, flight mechanics v2)

**Feasibility first.** A Python model of the adapter's panel aerodynamics
(MuJoCo venv) with programmed control, the 20 ms transducer and frame
latency showed the reduced-31-v1 crow could not fly: its 0.15 N m shoulders
let the lift fold the wings to the joint stop (the single tip panel then
acted ~0.3 m above the centre of mass and pitched the bird nose-up into a
stall), and the flat-plate polar (lift/drag < 4) cannot be flapped to level
flight within the 25 W budget at any kinematics (blade-element scan, thrust
< 0 whenever lift = weight). Changes, all in `make_crow`/`CROW_FLIGHT`
(`scripts/embodiment.py`) and the crow scene:

* *Air model* `scene.aerodynamics` = `quasi-steady-panel-v2`
  (`force::polar_panel_force`, read by `World::load`): three panels per wing
  at the box centres on each twist axis (inner, mid, outer), the tail fan,
  a vertical tail-base keel and isotropic body drag (CD 0.25 on the torso
  frontal area). Attached-flow wing polar: lift slope 4.6/rad (aspect ratio
  6.3), CD0 0.025, induced 0.063, stall drag only beyond CLmax 1.5. The
  keel (0.012 m^2, 0.19 m aft) gives the weathercock stability a banked
  crow needs: without it the body sideslipped into a spin in every turn.
* *Wing*: boxes span their own segment with a corvid's broad chord - span
  0.97 m, area 0.15 m^2 (~33 N/m^2); wing mass 0.034 kg each, proximal
  (the remainder moved to the torso, total 0.510 kg kept). Tail hinged 2 cm
  further aft.
* *Muscle and tissue*: shoulder flap 1.5 N m (half the weight at mid-span is
  ~0.5 N m, stroke reversal ~0.6 N m more), sweep 0.4, elbow 0.6, twist
  0.4, wrist twist 0.2 N m. A shoulder spring (0.79 N m/rad, rest 0.245 rad
  below horizontal) resonates the wing near 4 Hz and carries the mean lift,
  so the motors do aerodynamic rather than inertial work (the budget charges
  |torque x speed|, braking included). Structural wing, twist and tail
  joints get passive stiffness (co-contracted muscle and ligament): the
  brain's 20 ms transducer cannot stiffen them against stroke loads, so it
  sets their angles by torque against the spring. Wing joint damping 0.005
  (0.05 dissipated ~12 W at 4 Hz); legs 0.3 N m (0.06 let the standing crow
  fold onto its breast; 0.15-0.3 stand with a stretch reflex).
* *Launch and habitat*: hovering and take-off from standstill are outside a
  quasi-steady model (a drop from the 0.67 m perch reaches 3.6 m/s, half the
  stall speed), so the crow is released in level flight: spawn 3 m up near
  the south-west corner heading across the diagonal, `scene.launch` = 10.5
  m/s applied at load and every reset. A crow cruises at ~10 m/s and turns
  on a 15-30 m radius, so the woodland edge is an 80 m field bounded by an
  8 m tree line (perches, food and water unchanged at its centre).

Programmed control on this body (prototype, same panels and transducer)
flew level and through +-0.4 rad banked turns for 10 s with 10-20 ms of
added neural lag, using 13-16 W; blade-element scans give +0.5-0.9 N spare
thrust at 9-10 m/s with the bird polar against < 0 with the flat plate.

**Circuit** `scripts/build_crow_network_json.py` (71 cells, depth 0,
inherited cells): one bilateral wingbeat half-centre
(`add_leg_oscillators`, downstroke = stance with extra command drive, ~4.3
Hz, ~60 % downstroke) driving both pectorals in phase (per-wing angle servos
were unstable in the antisymmetric wing-body mode); stroke-end
proprioception from both shoulder angles ends each half-stroke at about
+-0.25 rad (the wing overshoots to about +-0.65); a tonic nose-up wing twist
with less on the upstroke; vestibulo-motor reflexes (roll -> differential
twist, pitch and pitch rate -> tail, flight path = pitch minus body angle of
attack -> twist, airspeed -> downstroke drive / tail); stretch reflexes for
neck, beak and legs; the shared hindbrain (MLR = flight command, RS bank
away from a darkening lower visual field, ARTR exploratory bank); and a
launch cell that holds the wingbeat and the exploratory bank off for ~0.5 s
after release. Design rules learnt on the way (all measured in the
runtime): (1) offsets cancelled against pulse-coded senses must go through
excitation-only relays (here every relay is referenced to `V_ref`, a relay
of the beak hinge velocity, which ramps up with the others over the first
~0.3-1 s - with tonic offsets the released crow rolled over in 0.4 s); (2)
with inherited dendrites a graded inhibitory synapse delivers ~0.72x when it
cancels equal excitation (1.0x alone); (3) the gyro channel (1/80 per rad/s)
needs a 15 ms relay to be useful; (4) the first downstroke after release is
twice as long (adaptation starts from rest) and must not run open-loop.
Open-loop (`celegans_depth_probe` on a synthetic level-flight trace with
roll, pitch, speed and flight-path steps): 4.07 Hz, 61 % downstroke,
downstroke/upstroke densities 0.73/0.50; tail density +0.94 for a 0.2 rad
nose-down step and -0.35 for 0.1 rad nose-up; the exploratory bank sets a
differential twist density of ~0.25 that alternates sides.

**Closed loop** (MuJoCo lock-step, 30 s, `target/qa/stage4/crow-flight/`:
`probe_embodiment_motion.py` with the new `airborne_*` metrics and m/s
target for fliers, `crow_flight.png` altitude/speed/attitude/path,
`crow_flight.mp4`): released at 10.5 m/s, the crow glides 0.5 s, then flaps
at 4.3 Hz (shoulder +-0.65 rad), dips to 2.1 m while the reflexes settle and
climbs to 5.2 m, airspeed 9.9 m/s median (9.2-10.6), pitch -15 to +5 deg;
exploratory banks of 20-35 deg turn it ~35 deg right, then ~85 deg left
to fly north. It stays aloft 10.6 s over a 97 m path (net 80 m) until it flies
into the north tree line; there it falls and lies flapping. Brain reply
4.4 ms median (p95 5.6 ms) on a host at load ~30. Earlier variants: wall
contact after 9-11 s in every 25 s run without visual steering; exploratory
banks from ARTR above ~3 mV/ms roll the crow over within 2-4 s.

**Limits.** Flights end at the tree line: the retina reports luminance
*changes* only, and a uniform dark wall moving down over uniform bright
ground changes each pixel once, so the darkness cells stay below 0.05
release up to contact (replayed traces) and RS never turns the crow; a
crow-scale turn (15-30 m radius) needs a looming signal at >= 30 m, i.e. an
edge or motion pathway, or a luminance channel in the IO contract. The
flight is a functional surrogate: quasi-steady panels (no unsteady lift,
wake or ground effect), a keel that real crows do
not have, passive joint springs instead of muscle co-contraction; other
engines still hard-code the v1 panels (Webots adapter, Unity, Unreal,
`sim/mujoco/runtime.py`) and must read `scene.aerodynamics`/`scene.launch`;
the graph export carries the joint springs, Webots and USD do not (Webots
cannot hold a spring rest away from the initial pose). The crow does not
take off, land, walk or hop: after contact it lies on the ground flapping.
The circuit was tuned with the inherited dendrite-active cells (like the
fish and fly); moving it to `Circuit(exact_cells=True)` needs a re-tune.

Reproduce the crow:

    target/robot-mujoco-venv/bin/python scripts/embodiment.py --profiles crow_corvus_corone
    python3 scripts/build_crow_network_json.py   # network_crow_flight.json + webots_world/configs/config_crow_flight.json
    # run_sim.sh --sim mujoco --robots crow_corvus_corone=1 --lockstep --headless --steps 15000 \
    #   --network-map crow_corvus_corone_0=network_crow_flight.json --config-map crow_corvus_corone_0=webots_world/configs/config_crow_flight.json
    MUJOCO_GL=egl target/robot-mujoco-venv/bin/python scripts/qa/render_trace_video.py TRACE.jsonl crow.mp4 --lookat-height 1 --distance 3 --elevation -12

#### Drosophila flight (`drosophila_banc`/`drosophila_fafb`, fly mechanics v3)

The fly takes off from standing and flies under its designed AARNN circuit
in closed loop (MuJoCo native adapter, lock-step).

**Body (fly mechanics v3, `scripts/embodiment_legacy.py`: `FLY_WING`,
`FLY_FLIGHT`, `fly_wings`, `fly_flight`, `fly_aerodynamics`).** Each wing is
hinged at its root on the thorax side, 12 mm above the thorax centre: a stroke
joint (`{side}_wing_flap`; +q moves the wing backward on both sides, so the
right axis is mirrored), a pitch (wing-rotation) joint (`{side}_wing_pitch`)
and an elliptical blade (span 6 cm, chord 2.5 cm, 0.05 g with hub) centred
on its pitch axis. Wing membranes do not collide. The legs keep the v2 link
masses (0.3 g): with flight-proportioned 0.05 g legs the unchanged walking
circuit flipped the light body within 0.5 s. Total mass 10.3 g. Two
findings fixed the geometry: a blade a quarter chord behind its pitch axis
produced stroke-amplitude-dependent pitching couples (~7e-4 N m per rad)
that no 50 ms loop can trim; and a passive pitch hinge weathervanes to zero
angle of attack in a quasi-steady model, so the hinge is driven.

**Air.** The blades carry the crow's `scene.aerodynamics`
(`quasi-steady-panel-v2`): one panel per blade at 70 % span on the pitch axis
(hinge torques otherwise kick the light body at each reversal), polar
`insect_wing` (lift slope 4.5/rad, CLmax 1.5, CD ~1.4 at 45 degrees as on
revolving insect wings). The old 0.0025 m2 panel on the hinge is gone.

**Flight motor (record `flight`, `resonant-flight-v1`,
`crates/aarnn-embodiment/src/flight.rs`, applied by `World::advance`).**
Asynchronous flight muscle beats the wings at the thoracic resonance, not a
neural rhythm (a 20 ms decoder cannot carry a 40 Hz wingbeat). One thoracic
phase `phi = 2 pi f t` (40 Hz: 12.5 steps of 2 ms per beat; at 20 Hz the
body rocked +-70 degrees within each beat) drives both wings in phase: the
stroke spring `k (c - q)` with `k = I (2 pi f)^2` from the compiled wing
inertia, plus a torque-limited drive towards `q* = c + A cos phi` (the stroke
joint's passive damping is the drive's derivative gain, implicit in the
engine, so the drive is stiff enough at 2 ms). The wing hinge is servoed to
`alpha0 tanh(q*'(phi + 0.5 rad)/w)`: leading edge up on both half-strokes and
turned over just before reversal (advanced rotation; timed from the measured
stroke it rotated ~5 ms late and lost a third of the lift). Neural commands
set power (recruitment: unpowered wings fold back to 1.45 rad and do not
beat), each wing's amplitude `A = g 1.2 a` and stroke centre
`c = g 0.4 u`. A stretch-activated van der Pol drive with per-side amplitude
regulation (the first prototype) let the wings drift out of phase and roll
the body; one shared phase is how the thorax couples real wings. Fixed-body
calibration: mean lift ~0.072 N at a 0.7 rad stroke, hover near 0.85 rad;
differential amplitude rolls (~9e-3 N m per unit asymmetry), a common centre
shift pitches (+0.2 rad back: 7e-4 N m nose down), a differential one yaws
(2.6e-3 N m per rad; left back/right forward yaws right).

**IO and readouts.** The contract keeps 418 sensors and 48 outputs, names and
order (the imported connectomes still bind). The flight commands
(`embodiment_neural.flight_commands`: `power`, `amplitude_left/right`,
`centre_left/right`) read tibia/tarsus agonist channels that the walking
circuit leaves silent, so walking never recruits flight; they decode with
8 ms (`decode_tau_s`; legs 20 ms): the attitude loop has little phase margin
and steering muscles are fast. The wing servos and their legacy seed pairs
(3, 17)/(11, 29) are retired. One IO change, so the IO digest changed:
sensor 22 (`dros_s_22_prox_12`, formerly the rear ray, unused by walking)
now looks down with a 0.3 m reach. It is the only height sense (flies use
ventral optic flow and ocelli); without it a lift trim of 0.008 rad of
stroke amplitude separates sinking from leaving the arena.

**Programmed proof** (`scripts/qa/fly_flight_proof.py`: exact Python ports of
the motor and panels on the generated model, the legs' servos and the 8 ms
decoder). Ideal controller: lift-off 0.12 s after recruitment, 13.9 s aloft
in 15 s at 0.160 m, attitude rms 0.3 degrees. Controller on brain-visible
sensors only (gyro, accelerometer, ventral ray, ~50 ms of cell, synapse and
transport lags): 13.8 s aloft at 0.163 m, rms 1.5 degrees, max 17.6. It needs
three things: haltere rate plus a leaky (1.5 s) integral of rate as the
attitude estimate; drift feedback from the accelerometer, which in flight
senses drag, not tilt (without it every run rolled over within 3 s); and
height-rate damping. With 3 % white command noise it lost attitude after
~10 s, and 20 ms of extra delay is enough to destabilise it.

**Circuit** (`scripts/build_drosophila_flight_network_json.py`, exact
cells, 29 cells): `TAKEOFF` (slow motivation) latches `FLIGHT` (30 mV
plateau; power, baseline amplitude, legs tucked with both femur sides
matched for the unequal readout coefficients); excitation-only gyro relays
(6 ms) with two 1.5 s integrators read against an identical input-free
`I_ref`, so the integrators' 3 s start-up cancels; push-pull pairs `ROLL`,
`PITCH`, `YAW`, `LIFT` (2 ms) on the flight outputs (2 ms); lateral drift
from a 100 ms twin relay of the accelerometer; height from the ventral ray
with a climb transient (fast minus slow twin relay through an inverting
cell: cancelling a large weight by direct inhibition tripled the height
gain in a conductance membrane); the shared hindbrain (`RS` turns through
`YAW`). Calibration happens in the runtime at build time (open-loop probe):
every steering cell's tonic is trimmed to its operating point in level hover,
then each path's small-signal gain is measured and rescaled to the proof's
gains (the designed weights delivered 0.4-1.6x of them). Takeoff waits until the
integrators have settled (~4 s).

**Closed loop (lock-step, `network_drosophila_banc_flight.json`).** 25 s
run: lift-off at 4.19 s, then 20.8 s of continuous flight to the end; height
0.164 m mean (5-95 %: 0.11-0.19 m; set point 0.15); mean |roll| 2.9 and
|pitch| 6.5 degrees, attitude rms 9.1 (p95 16.4); wingbeat 40.0 Hz, stroke
0.89 rad; the fly crosses 0.81 x 0.65 m of the 1.1 m arena (~5.9 m of path,
0.1 s means); median exchange latency 3.0 ms (p95 3.7). FAFB (same body,
own network `network_drosophila_fafb_flight.json`): 15.8 s continuous flight
in a 20 s run, 0.162 m, rms 9.5 degrees. Video:
`target/qa/stage4/fly-flight-f17-20s/drosophila_flight_aarnn.mp4`.
Walking re-verified on the v3 body with the unchanged
`network_drosophila_banc_walk.json`: 0.373 m in 15 s with a 3.73 Hz tripod
(v2: 0.39 m, 3.7 Hz); it walks into fruit_2 at ~5.6 s and rears over at
7.2 s.

**Tests.** `aarnn-embodiment` flight unit tests (folded and silent when
unpowered, mirrored and in phase, malformed declarations fail closed);
adapter `fly_flight_motor_lifts_off_only_on_a_neural_command` (no brain:
wings folded, fly standing; graded commands through the record's readouts
and the shared decoder: 40 Hz in-phase strokes of ~1.02 rad and lift-off).

**Limits.** Flight is MuJoCo-native only: Isaac no longer applies the stale
v1 hinge panels to the v3 body but has no motor or panel port (wings
passive); the Webots adapter, `sim/mujoco/runtime.py`, Unity and Unreal do
not apply the motor or the panels, and the Webots/USD exports carry the wing
joints without the thoracic spring. No wall avoidance in flight: the
range rays reach 8 cm, the fly bounces along walls with pitch swings of
20-40 degrees there, and a ray-driven tilt-away reflex (`--wall-pitch`,
`--wall-roll`) tumbled the fly at 0.06 and is off. The forward accelerometer
saturates on the wingbeat recoil (beyond +-20 m/s2), so fore-aft drift is
not sensed. No landing, and walking and flight are separate networks. The
latency margin is small (see the proof). Quasi-steady aerodynamics (no
unsteady lift, wake or ground effect) on an illustrative 5 cm, 10.3 g fly.

Reproduce the fly:

    target/robot-mujoco-venv/bin/python scripts/embodiment.py --profiles drosophila_banc drosophila_fafb
    target/robot-mujoco-venv/bin/python scripts/qa/fly_flight_proof.py --seconds 15        # programmed proof
    target/robot-mujoco-venv/bin/python scripts/build_drosophila_flight_network_json.py   # network_drosophila_banc_flight.json + config (~2 min: runtime calibration)
    # run_sim.sh --sim mujoco --robots drosophila_banc=1 --lockstep --headless --steps 12500 --trace OUT --trace-every 5 \
    #   --network-map drosophila_banc_0=network_drosophila_banc_flight.json --config-map drosophila_banc_0=webots_world/configs/config_drosophila_banc_flight.json
    MUJOCO_GL=egl target/robot-mujoco-venv/bin/python scripts/qa/render_trace_video.py TRACE.jsonl fly.mp4

### Showcase videos, 3D layouts and the spatial ladder (2026-11-23)

**Showcase videos.** `target/showcase-videos/*.mp4` (README there): one per
robot (C. elegans, zebrafish, Drosophila flight, hexapod, NAO, UGOT
quadruped, UGOT balancer, crow, Keenon T8), 1080p, real time. Left: the
MuJoCo body replaying a lock-step closed-loop run (`--trace-every 1`).
Right: the real `aarnn_rust --ui` stepping the same network through the
same recorded sensory stream (replay capture, `ui_screenshot` feature,
`NM_UI_REPLAY_*`), so each frame shows the network state that produced the
body's next motor command. The network pane glows sensory/inter/motor cells
by their spikes in each 20 ms window (or graded release), shows five
auto-chosen membrane traces (most variable cells, named from
`connectome_labels`) and the input/output rasters. Capture fixes in
`src/ui.rs`: spikes tallied from `step_sync`'s `StepOut`; the idle sim
thread's snapshot no longer overwrites activity, rasters and probes during
replay; overlapping header texts moved; per-lane auto-scale. Xvfb must run
with `-noreset` (else the pointer resets to the screen centre and hovers
the graph). Pipeline: `target/qa/showcase/{record,rerender,make_all}.sh`.

**Biologically equivalent 3D layouts.** Designed circuits put every soma at
the origin (`aarnn_circuit._net` also switched morphology and delays off),
so spatial mechanisms were untestable. `scripts/layout_designed_circuit.py`
places every node at its soma in the C. elegans import's normalised body
frame (y rostral, x left, z dorsal): zebrafish (nMLF, hindbrain RS and ARTR,
8 spinal segments of E/A/C cells, ventral motor columns, lateral-line and
statoacoustic ganglia), hexapod (one thoracic hemiganglion per leg, motor
neurons in the same ganglion, insect leg sensilla in the leg), NAO as a
human-equivalent nervous system (basal-ganglia locomotor command, MLR,
medullary reticulospinal, pontine turn oscillator, lumbar CPG, cervical and
lumbosacral motor pools, dorsal-root and cranial ganglia). With layout,
morphology and conduction delays on (S1) every designed circuit still
works: zebrafish 0.69 vs 0.63 L/s at 4.8 vs 4.5 Hz, hexapod 0.073 vs
0.059 L/s at 2.3 vs 1.9 Hz, NAO 0.586 vs 0.582 m in 30 s; none fall.

**Spatial ladder** (`complexity_ladder.py --network <layout> --spatial ...`,
assemblies = region labels for designed circuits, anatomy for the worm).
Levels: S0 no morphology/delays; S1 frozen anatomy + delays; S1r + global
resonance drive (0.5, ambient 0.2); S1g + local proximity coupling (somas
within 0.05 body units pull towards each other's membrane potential,
`aarnn_gap_junction_radius`); S2/S2dev + growth (maintenance/developmental
pruning); S3 growth + resonance. Closed-loop behaviour (10 s):

| Level | worm (T1) undulation / forward fraction | zebrafish (T4) path / speed |
|---|---|---|
| S0 | 0.8 Hz / 0.64 | 5.7 L / 0.32 L/s |
| S1 | 0.8 Hz / 0.66 | 6.6 L / 0.37 L/s |
| S1r | 1.0 Hz / 0.55 | 7.8 L / 0.37 L/s |
| S1g | 0.8 Hz / 0.65 | 6.4 L / 0.34 L/s, tail beat 5.5 Hz |
| S2 | 0.8 Hz / 0.69 | 1.2 L (swimming lost) |
| S2dev | 0.6 Hz / 0.43 | 2.0 L |
| S3 | 1.0 Hz / 0.74 | 1.4 L |
| any +L (learning) | 0.1 Hz | 1.9-3.2 L |

The worm tolerates and slightly benefits from growth with resonance (S3);
the designed fish loses swimming as soon as growth is on, while resonance
alone helps it. Learning (spike STDP/Hebb/Oja, graded plasticity off) still
degrades every level. Open-loop resonance (coherence, phase spread between
assemblies) and capacity (8-pattern discrimination, chance 0.125) results:
`target/qa/complexity/<robot>/SPATIAL_ISO.md`. So far:

* zebrafish (8 spinal-segment assemblies): S1 gives a 9.75 Hz rhythm with
  fully coherent segments (coherence 1.0) spread in phase (0.81, a travelling
  wave) and capacity 0.46. Proximity coupling (S1g) sharpens the rhythm
  (peak-power fraction 0.92 -> 0.999, 11 Hz) but lowers capacity to 0.31;
  the global resonance drive (S1r) breaks coherence (0.14) and capacity
  0.33. Learning slows every level to ~0.5 Hz; no controlled conditioning;
  only S1g+L gains capacity within a run (0.29 -> 0.33).
* hexapod (3 thoracic-ganglion assemblies): S0 capacity 0.52; with anatomy
  (S1, S1r, S1g) capacity falls to 0.04-0.13 while closed-loop walking stays
  within 30 % of the reference. Not lost wiring (24/24 input routes in
  both); with anatomy the delay history grows from 1 to 5 ms and the leg
  sensilla sit 0.2-0.8 body units from their ganglia, so the long peripheral
  afferents' delay/attenuation is the likely cause (to check).
* NAO (T0, human-equivalent layout; assemblies: lumbar CPG, delay cells,
  turn oscillator, auditory nuclei): every level without learning walks,
  every level with learning falls. Capacity 0.40 (S0), 0.33 (S1), 0.33
  (S1r), 0.27 (S1g); S1r pushes the network to 2.1 Hz with low coherence.
* C. elegans (T1, anatomical ganglia as assemblies, real connectome):
  without learning S0/S1/S1g are nearly identical (coherence 0.07-0.08,
  dimensionality 6.5, capacity at or below chance - the probe channels
  barely separate); S1r raises the habituation index (0.19 vs 0.08).
  Learning drives the connectome into lock-step hypersynchrony: S0+L
  coherence 0.98, phase spread 0.003, dimensionality 1.1, no crawl. With 3D
  anatomy the collapse is partly resisted: dimensionality 3.2 (S1+L), 4.1
  (S1r+L), 4.2 (S1g+L), coherence 0.76-0.78, and S1r+L / S1g+L still crawl.
  Learning also drives ~18 % of recurrent synapses to zero (2806 -> ~2290).
  *Correction (later on 23 Nov):* this collapse was an artefact. Learning
  created synapses that do not exist (see "Learning without losing innate
  skills" below); with that fixed, no level hypersynchronises, so this
  result is not evidence for or against the resonance hypothesis.
* In these designed circuits, spatial coupling raises synchrony at the cost
  of representational capacity; it has not yet shown the learning gain the
  resonance hypothesis predicts. The circuits were designed and tuned
  without space, plasticity is off by design and the learning rules are
  still destructive - so this tests the present mechanisms, not the
  hypothesis itself.

**Growth is unbounded and its cost explodes.** On the 55-cell fish, frozen
anatomy (S1) runs at 1.0 ms per 2 ms frame; with growth (S2) the cost per
frame keeps rising (24 ms after 0.1 s, 52 ms after 0.4 s, ~580 ms after
2 s). In 300 ms of simulated time the ~120 cells sprout ~1,450 axon and
~940 dendrite branches and make ~335 synapses (`NM_TRACE=1`) - tens of
sprouts per cell per second, where real arbors remodel over minutes to
hours. Hot spot: `Morphology::seek_energy_biased` -> `energy_at` for every
growing tip (15 samples each) over an ever larger field. This swamps the
designed circuit (swimming lost) and makes growth studies infeasible.
Lowering the network's `dendrite_sprout_prob` 100x changed neither the
sprout counts nor the cost, so that field is not what drives this sprouting
- the effective control still has to be found. One unit error was fixed on
the way: sprouting and trunk/branch growth were multiplied by the cell
type's raw `synaptic_gain` (a weight scale: 100-500 in designed circuits),
now by its gain relative to the network's base gain
(`morphology::growth_gain_factor`; base-1 connectome imports unchanged).
Biological growth is bounded by competition for neurotrophic factors and by
metabolic cost; the next step is a biological sprouting timescale plus a
per-neuron branch/bouton budget, not a faster field.

**Other fixes.** C. elegans S0 (no morphology) hung distributed-node
start-up: `Runner::new` always rebuilds a morphology from the weights
(`Morphology::from_weights`), which never finished for the 302-cell graft;
the ladder now sets `NM_REALTIME_DISABLE_MORPHO=1` for variants without
morphology. The ladder's open-loop summary line crashed on the per-seed
lists (the main thread died while workers kept computing unrecorded
results); fixed, and `--resume` skips variants already in the report.
UGOT balancer re-tuned for the OEM wheels (see the UGOT section).

### Learning without losing innate skills; networks shown in 3D (2026-11-23)

**Why learning destroyed behaviour.** Long-term plasticity proposed an
update for *every* entry of the dense weight matrices, and the commit
clamped every entry into `[w_min, w_max]`. Absent synapses (weight 0) were
potentiated whenever a sensory spike preceded a hidden spike, so learning
wired up the connectome itself. On the worm (T1/S0+L, open loop on the
native run's senses), the input matrix grew from 246 to 12,381 of 14,496
entries in 8 s. The output matrix grew from 273 to 6,742, and 88 of the 96
inhibitory neuromuscular synapses vanished: depression drove a magnitude to
-0.0, and the next update then treated it as excitatory. Every cell got the
same drive, and all 302 fired at ~320 Hz in lock-step (chi 0.58,
dimensionality 1.1). Neuromodulation and graded plasticity were not the
cause; switching each off left the runaway.
Data: `docs/research/poster_v3/data/celegans_learning_runaway.json`.

**Fix (all engines).** `plasticity::apply_weight_delta`:
- a weight of exactly 0 stays 0 (creating synapses is growth's job);
- an existing synapse keeps its sign and at least `SILENT_SYNAPSE_FLOOR` = 1e-6 (LTD silences, pruning removes);
- an imported efficacy above `w_max` is never truncated (it can only be depressed into range).

The OpenCL `plasticity_update` kernel mirrors this, and also stops clamping
inhibitory synapses to zero. `runner::matrix_plasticity_proposals` proposes
updates only for existing synapses onto cells that learn.

The single-hidden-layer gap is closed: every hidden layer now learns its
recurrent synapses, the last layer included, on both the reference and the
accelerator paths. Before, a 1-layer connectome never learned its recurrent
wiring.

**Homeostatic counterweight** (`NetworkConfig::aarnn_rate_homeostasis_*`,
`plasticity::RateHomeostat`, `homeostatic_scale_rows`, on by default while
plasticity is on):
- Each cell's firing rate is averaged (tau 1 s).
- After a 2 s warm-up, the cell keeps that rate as its own set point (or a common `target_hz`).
- Every 10 ms the synapses onto the cell are scaled multiplicatively: excitation by `exp(k e)`, inhibition by `exp(-k e)`, with error `e = (set - rate)/set` in [-1, 1] and `k = gain * dt / tau`.

Biology: Turrigiano et al. 1998 (multiplicative scaling), Kilman et al. 2002
(inhibition scales the other way), Hengen et al. 2016 (cell-autonomous set
points), Zenke & Gerstner 2017 (Hebbian learning needs compensatory
processes). The state is saved in snapshots.

Worm with learning, 40 s:

| Variant | Rate | Dimensionality | Inhibitory synapses |
|---|---|---|---|
| Structural fix only | 10.2–10.5 Hz | 131–134 | all kept |
| Fix + homeostat | 9.8–9.9 Hz | 123–125 | all kept |
| No learning | 9.5–10.1 Hz | 123–134 | n/a |

**Learning only in specialised cells** (the user's proposal: gait is innate,
learning belongs to specialised circuits; newborn stepping already has the
locomotor primitives that are kept for life, Dominici et al. 2011). The mask
is `NetworkConfig::aarnn_plastic_cells` (hidden and output masks). Only
synapses onto marked cells learn and are scaled; the bouton overlay
honours it too, and a mask forces the reference plasticity path. The ladder
adds a `+Ls` variant and the `PLASTIC` lists in `complexity_ladder.py`:
- worm: the amphid interneurons AIA/AIB/AIY/AIZ (Gray et al. 2005; salt memory at ASER->AIB, Kunitomo et al. 2013);
- NAO: its sonar relays and avoidance/back-up cells;
- zebrafish: its hindbrain command/ARTR cells.

Closed loop (`target/qa/complexity/{celegans,nao}/spatial_learn.json`):

| Robot / level | no learning | every cell learns | specialised cells learn |
|---|---|---|---|
| Worm S0 (L/s, forward fraction) | 0.126, 0.64 | 0.034, 0.53 (was 0.003 before the fix) | 0.125, 0.64 |
| Worm S1 3D anatomy | 0.146, 0.66 | 0.015, 0.52 | 0.135, 0.66 |
| Worm S1g + gap coupling | 0.146, 0.65 | 0.011, 0.57 | 0.146, 0.64 |
| NAO S0 (path in 30 s) | 16.0 L, upright | 4.4 L, falls | 15.7 L, upright |
| NAO S1 | 16.0 L, upright | 8.1 L, falls | 16.0 L, upright |

With every cell learning, the worm keeps its rhythm (0.7-0.8 Hz; before the
fix it fell to 0.1 Hz) but its muscle drive halves or worse. With
specialised learning, its gait matches no learning. NAO open loop
(`SPATIAL_LEARN.md`): `+Ls` keeps the network rhythm (0.75 Hz, phase
spread 0.80 vs 0.83 without learning). No variant shows controlled
conditioning (index ~0): the learning cells are protected, but a learning
gain is still to be shown.

Worm open loop (`target/qa/complexity/celegans/SPATIAL_LEARN.md`, T1/S0,
2 seeds) confirms the hypersynchrony is gone.

| Variant | Coherence between ganglia | Phase spread | Dimensionality | Conditioning index |
|---|---|---|---|---|
| Before the fix | 0.98 | 0.003 | 1.1 | n/a |
| Every cell learns (after fix) | 0.071 | 0.53 | 5.9 | -0.10 |
| Specialised subset learns | 0.073 | 0.65 | 6.5 | -0.02 |
| No learning | 0.072 | 0.65 | 6.5 | 0.045 |

So the "learning-driven hypersynchrony" was the synapse-creation artefact,
not a property of the learning rules. No variant conditions.

**Networks shown in their 3D biological arrangement.**
- `scripts/layout_designed_circuit.py` gained profiles for every showcase body:
  - `drosophila(_banc)`: imported BANC cells at their annotated top neuropil and soma side, from `data/drosophila/BANC v626/neurons.csv.gz`; designed flight cells at their functional equivalents;
  - `ugot_quadruped` and `keenon_t8`: mammalian cord, fore limbs and wheels in the cervical enlargement, hind ones in the lumbar;
  - `ugot_balance`: vestibular nuclei, Deiters' nucleus, Clarke's column, wheels as lumbar pools;
  - `crow`: optic tectum, vestibular nuclei and vestibulocerebellum, cuneate nucleus for airflow, brachial wing CPG, lumbosacral legs, caudal tail, tracheosyringeal nucleus, ventral respiratory group, retinotopic retina, tonotopic cochlear ganglion.
- Outputs: `target/qa/spatial/showcase/network_<robot>_layout.json`.
- Moving the somas leaves the dynamics bit-identical for these circuits: gap-junction strength is 0 and morphology and delays are off, and the zebrafish, NAO and crow hidden series are identical with `cmp`.
- The UI had anatomical stages (4 pixels, 5 straight, 6 branching connections), but replay capture never handed the runner's topology to the renderer: the idle sim thread publishes topology only while stepping a growing network. The capture therefore fell back to stage 3, the synthetic layered graph. `ReplayCapture` now publishes the topology and requests the edge cache.
- New capture camera controls: `NM_UI_CAMERA_YAW_DEG/_PITCH_DEG/_ROLL_DEG` and `NM_UI_CAMERA_STRETCH` (vertical exaggeration). Roll -90 with yaw 90 lays a body's rostro-caudal axis horizontally, head left and dorsal up; `anatomy::Frame` takes the roll too.
- All nine showcase videos were recaptured on 23 Nov with this view. `target/qa/showcase/make_all.sh` uses the layout networks, `capture_ui.sh` uses `STAGE=6`, and per-robot `CAM` sets yaw 60 (oblique, so left and right separate), roll -90, and zoom/stretch so the layout fills the panel. The previous stage-3 scripts are kept as `*.stage3.sh`.
- Placement alone leaves the dynamics bit-identical for all eight designed circuits (`cmp` of 4 s hidden series).

**Neuron-count scaling** (`examples/aarnn_scaling_probe.rs`,
`scripts/qa/scaling_study.py`; results in `target/qa/scaling/summary.json`).
- Networks: random recurrent AARNN networks with worm cell physiology and fixed in-degree (20 recurrent, 4 input synapses, 20 % inhibitory, Dale), so only N changes.
- Task: 32 random patterns over 32 channels, 6 noisy presentations each.
- Decoder: leave-one-out nearest centroid on each cell's spike counts, while the pattern is on and 0-100 ms after it ends.
- Two defaults had to be switched off for the probe to be valid: `morpho_growth_enabled`, whose migration rewired inputs even with morphology off, and ambient energy and the default physiology, which made cells fire at ~40 Hz with no input. Base physiology is the worm's config.
- A third construction fix: the probe builds with `p_in/p_hidden/p_out` = 0. With the worm config's `p_hidden` = 1, `Runner::new` first builds a dense N x N network and its morphology, which took hours at N >= 256.

Results (3 seeds; `target/qa/scaling/summary.json`):

| Neurons | 8 | 32 | 128 | 512 | 1,024 |
|---|---|---|---|---|---|
| 32-pattern decoding (chance 0.03) | 0.08 | 0.21 | 0.31 | 0.39 | 0.42 |
| 8-pattern decoding (chance 0.125) | 0.18 | 0.43 | 0.59 | 0.64 | 0.70 |
| effective dimensionality | 6.9 | 20 | 39 | 53 | 54 |

32-pattern decoding rises monotonically, about 5 points per doubling of N. Firing stays at 13-14 Hz at every size. Decoding 0-100 ms after the input ends stays at chance at every size: random wiring gives no fading memory in this regime. At 2,048 cells: 0.45 (32 patterns), 0.74 (8 patterns), dimensionality 59.

**Is the network more than the sum of its parts?** (The user asked for this
on 23 Nov: does storable information grow faster than neurons are added?)
Neurons were added one at a time, N = 1..64 (3 seeds). Each N was run twice
with identical cells, input wiring, patterns and noise: recurrent synapses
on (all-to-all below 21 cells, then 20 per cell) or off.
Outputs: `target/qa/resume/run_1by1.sh` -> `target/qa/scaling_1by1/`,
analysis `scripts/qa/scaling_1by1.py`, statistics in `stats.json`.

| Neurons | connected | disconnected | connected better | sign test |
|---|---|---|---|---|
| 1-16 | 0.074 | 0.075 | 18/38 | p = 0.87 |
| 17-32 | 0.151 | 0.125 | 36/45 | p = 7e-5 |
| 33-48 | 0.191 | 0.161 | 37/44 | p = 5e-6 |
| 49-64 | 0.228 | 0.200 | 33/45 | p = 0.003 |

The gap grows with N (+0.005 per 10 neurons, permutation p = 0.001).
Information per neuron (Fano lower bound from accuracy) rises 1.8x from
1-16 to 17-32 neurons in the connected network and then holds. The
disconnected cells gain 1.15x. So above ~16 cells the connections make
added neurons contribute more than they do alone.

This is the representation of a present stimulus, not storage of
experiences. Storage that grows faster than the cell count is the
classical associative-memory result: memories live in the N(N-1) possible
synapses, approaching ~0.69 bits per binary synapse for sparse patterns
(Willshaw et al. 1969; Palm 1980).

`examples/aarnn_storage_probe.rs` (work in progress) tests it in AARNN:
- Storage: clipped Hebbian synapses between the members of sparse assemblies.
- Retrieval:
  - a half cue;
  - a best-match readout;
  - a global inhibitory reset pulse between tests.
- What works: at N = 80, with 8-cell assemblies, association 20 and lateral inhibition 4, one stored experience is recalled from a half cue in every test.
- What doesn't yet: at higher loads, assemblies that share cells ignite each other as self-sustaining attractors.
- Fixes needed before its capacity can be measured against N:
  - stimulus rates of 100 Hz, not 500 Hz (input short-term depression);
  - deterministic release (worm config 0.72) while debugging;
  - a 2-spike activity threshold (cells fire ~1 Hz spontaneously);
  - an inhibitory interneuron pool for activity-dependent competition.

**Storage with an inhibitory interneuron pool (done 23 Nov, afternoon).**
- Pool: interneurons are 20 % of cells (`--interneurons 0.2`); every excitatory cell excites every interneuron and every interneuron inhibits every excitatory cell (Dale's law holds).
- The pool weights scale 1/N relative to 80 cells (`--pool-ref 80`). With fixed weights, total inhibition grew with N and capacity fell from 14-17 (N = 80) to ~2 (N = 256-320).
- Recall uses a brief cue volley (12 ms at 1 kHz drive) to half an experience, read over 40 ms. That restores the coincidence threshold: with long cues, one active partner fired a cell, so overlapping assemblies ignited each other (capacity <= 2 at every setting tried).
- Drivers: `target/qa/resume/run_storage*.sh`. Report: `scripts/qa/modular_storage_report.py` -> `target/qa/modular_storage_report.json`.
- 3 seeds per size, 8-cell experiences:

| Excitatory neurons | 16 | 40 | 80 | 128 | 192 | 256 | 320 |
|---|---|---|---|---|---|---|---|
| experiences recalled (>= 90 %) | 2.3 | 7.7 | 16.7 | 27 | 44.7 | 65.3 | 77.7 |
| bits stored per neuron | 2.0 | 5.0 | 7.2 | 8.5 | 10.5 | 12.4 | 12.4 |

Experiences grow ~N^1.15 and stored information ~N^1.56 (log-log fit over 19 sizes). Information per neuron rises 6x from 16 to 320 neurons: in this test, the stored information grows faster than the number of neurons. Bits = experiences x log2 C(N, 8), the information needed to name each experience's cells.

**Groups of groups (the user's proposal: groups grow to a threshold size, the next neuron starts a new group, groups form collections of a threshold count, simple attachments between groups, sparse wiring inside groups).**
`aarnn_scaling_probe --module-size G --within-p p --branching B --attach 2` wires groups inside (all-to-all, or each pair with p). Every pair of groups in a collection of B groups gets 2 excitatory synapses each way, recursively for collections of collections.

Representation results (32-pattern decoding, paired over seeds):
- N = 20-640: groups of 20 match the interwoven clump with the same synapse budget (+3 %, 26/41, p = 0.12, not significant); groups of 12 (below the ~16-20 partner threshold) are 15 % worse than groups of 20 (34/37, p = 1e-7); both beat disconnected cells by 25-27 %.
- N = 320, sparse groups (p = 0.5): accuracy rises with partners per neuron: G40/60/80 give 0.39-0.41 / 0.42 / 0.47. Clumps with 20/30/40 partners give 0.35 / 0.46 / 0.49. Beyond the threshold, partners per neuron matter more than grouping.
- Collection size B = 2/4/8 made no detectable difference at 320 cells.

Storage results (N = 160):
- Open network: 43 experiences.
- Full groups of 80: 28 (8-cell) and 12 (16-cell, against 22 open).
- Sparse groups of 80 at p 0.5: 7 (16-cell; 8-cell assemblies fail because too few coincident partners remain).

Storage tracks the number of possible synapses. In these runs experiences were placed in random groups, not filled group by group as the proposed growth rule would do. That rule, and larger collections, are the next tests.

### NAO learns obstacle avoidance by experience (2026-11-23)

Learning uses the same AARNN algorithms as every other network. Only the NAO's wiring and parameters are specific to it.

**Circuit** (`scripts/build_nao_learn_network_json.py` on `build_nao_walk_network_json.py --learn-groups 20`):
- The innate far-sonar turn of the walking NAO is removed.
- In its place, each chest sonar feeds a group of 20 distance-tuned relay cells (`CS_{l,r}_00..19`, onsets 0.45-1.5 m).
  - The group is sparse inside: each ordered pair is connected with p = 0.5.
  - The two groups form one collection, joined by a 2-synapse attachment (the groups-of-groups rule).
  - Anatomy: inferior colliculus.
- Every relay cell has a silent synapse (0.05 mV/ms) onto that side's learning-turn cell LT (superior colliculus). LT drives the reticulospinal turn cell exactly as the innate cell did.
- The innate near reflex (sonar < 0.4 m: back off and turn) also excites LT, so it is the teacher (distributed adaptive control).
- Plastic-cell mask: only synapses onto LT learn. The homeostat (tau 600 s) is the counterweight.
- Neuromodulation is off, so the learning rate is plain.

**Depth matters.**
- Point cells (S0) have only matrix STDP. STDP cannot learn this, because the relay spikes do not cause LT's spikes, so its potentiation and depression cancel (12 paired approaches: turn onset unchanged at 0.27 m).
- The association is learned by the dendritic-bouton plasticity overlay that S1 (3D anatomy and morphology) adds. It acts on graded co-activity through its Hebbian and Oja terms.
- Ablation (`ol8_nohebb_paired`, Hebbian share 0): the network still learns, more slowly (turn onset 1.06 / 0.98 m and weights x8.2 / x7.2 after 8 approaches per side, against 1.30 / 1.29 m and x14). The Hebbian share roughly doubles the rate; it is not the only learning term.
- This overlay now uses the graded cells' activity, not spike flags (`src/runner.rs`).
- `aarnn_plasticity_eta` (default 0, which means the runtime rate) lets a network set its rate as a parameter.

**S1 calibration.**
- Designed circuits inherit the C. elegans template's distance attenuation (0.26 per mean path) and release heterogeneity (0.12). These silenced the NAO's threshold-tuned near reflex at S1: relay 2.3 Hz against 13.7 Hz in point cells.
- The `--route-report` option of `celegans_depth_probe` found the two causes:
  - distance attenuation;
  - a lower release probability on the right afferent synapse.
- Rule (the same for any animal): the largest equal values at which the innate reflex keeps its point-cell level on both sides. Here that is 0.04 / 0.04; 0.06 lost the right side (sweep in `target/qa/nao_learn/cal_*`).
- Both features stay on. For comparison, the NAO connectome builder uses 0.08 / 0.08.

**Open-loop proof.**
- Script: `scripts/qa/nao_learn_openloop.py`; report: `scripts/qa/nao_learn_report.py`; runs in `target/qa/nao_learn/ol6_*`.
- 16 scripted wall approaches, alternating sides: 2.2 m -> hold -> retreat over 30 s.
- Turn onset uses one absolute threshold for every condition: half the frozen network's reflex peak.
- `--watch-rows LT_l,LT_r` logs the synapses onto LT in every 50 ms window.

| Condition (same network) | turn onset, approaches 1-2 -> 15-16 | LT activity at 0.6-1.2 m | summed sonar -> LT efficacy |
|---|---|---|---|
| reflex paired with approach (hold 0.25 m) | 0.35 m -> 1.30 m | 0.4 -> 17.7 Hz | x14 |
| approach only, never near enough for the reflex (hold 0.5 m) | never turns | 0.4 -> 3.7 Hz | x4 |
| frozen (plasticity off) | 0.30-0.35 m throughout | 0.4 Hz throughout | unchanged |

- With the reflex as teacher, the network learned to turn from 1.3 m, more than three times the innate range, within 7-8 experiences per side. Both sides learned.
- Without the teacher, the same exposure gave a slow, non-associative potentiation, not a turn. It accelerates: the Hebbian term reinforces itself as LT's activity rises, and the 600 s homeostat does not hold it within 16 approaches. With much longer exposure it could cross the turn threshold. This is the open caveat.
- A faster homeostat is not the fix (`target/qa/nao_learn/ol7h60_*`, tau 60 s):
  - it left the approach-only drift unchanged (summed efficacy 0.38 vs 0.37);
  - it suppressed paired learning (0.31 vs 1.25);
  - because it scales every excitatory input to LT, it also weakened the teacher synapse: the innate reflex turn dropped out on 3 of 16 approaches.
  - The counterweight must stay much slower than learning, as biological homeostasis (hours) is slower than learning (minutes).
  - An LTD term for unpaired activity is the next test.

Figure: `nao_experience_figure` in `docs/research/poster_v3/figures.py`. The poster now uses the teacher-gated final network instead: `nao_gated_learning_figure` (open loop `target/qa/nao_learn/ol14_report.json`; closed loop `closedloop_summary.json`).

**Teacher-gated (three-factor) plasticity: a new shared algorithm (23 Nov, evening).**
In conditioning, an unconditioned stimulus activates neuromodulatory neurons (dopaminergic, noradrenergic), and their release gates synaptic plasticity: three-factor learning. Co-activity alone does not write a memory.

AARNN had only network-wide neuromodulators driven by global signals; no cell could release one. The new mechanism is available to every network, with no change unless it is configured:
- `aarnn_modulatory_cells` (a cell mask) marks the cells that release a modulator.
- Their mean release rate, cleared with `aarnn_modulator_tau_ms` (1 s), sets the modulator level.
- Long-term plasticity runs at `eta x level / aarnn_modulator_ref_hz` (10 Hz), at most x3.
- With no modulatory cells, plasticity is ungated as before. The homeostat is not gated.
- Code: `Runner::modulated_plasticity_gate`, which multiplies the single learning rate used by matrix STDP, the dendritic-bouton overlay and the GPU path.
- Test: `teacher_gated_plasticity_follows_the_released_modulator` (gate 1 when ungated, 0 for a silent teacher, rising with release and capped at 3, clearing with tau).

NAO wiring (`build_nao_walk_network_json.py --learn-teacher 1`):
- The innate near reflex (N_sonar cells) also drives a noradrenergic locus coeruleus cell `LC`, laid out in the dorsal pons.
- `build_nao_learn_network_json.py --gated 1` marks LC as the only modulatory cell.

Open loop, 16 approaches (`target/qa/nao_learn/ol10_*`, `ol11_*`; 72-cell network `network_nao_teach_1`):

| Condition | onset, approach 1 -> 15 / 16 (left / right) | summed sonar -> LT efficacy |
|---|---|---|
| paired, gated, homeostat on | 0.36 -> 1.25 m / 0.35 -> 0.72 m | x11 / x5.8 |
| paired, gated, homeostat off | 0.36 -> 1.26 m / 0.35 -> 0.38 m | x12 / x4.4 |
| approach only, gated, homeostat on | never | x2.1 |
| approach only, gated, homeostat off | never | x1.00 (unchanged, as frozen) |
| frozen | 0.35-0.36 m | x1 |

- The gate removes exposure-driven potentiation completely. The x2.1 left with the homeostat on is not learning: the LC teacher was silent (0.02 Hz, gate 0.002). It is the homeostat scaling up the inputs of the turn cell, which is quiet most of the time; its set point, taken in the first 2 s, does not represent a cell that is active only near walls.
- The gated NAO network therefore runs with the homeostat off (`--homeo-gain 0`, the new default). The teacher gate gives the stability the homeostat was added for. A set point for episodically active cells is an open item.
- The right side learns more slowly because its innate reflex, and so its teaching signal, is weaker (right relay synapse with a lower release probability; see the S1 calibration above).

**Closed loop: why the robot was trapped, and the fix.**
- Baseline: in MuJoCo (3 m x 3 m walled room, `scripts/qa/nao_learn_closedloop.py`, `target/qa/nao_learn/cl_*`) both the learning and the frozen network were trapped at the first wall. They rocked at 0.3-0.4 m for 460-750 of 800 s, and the frozen network never left.
- Cause: the turn shortens one side's stride. Walking backwards in a back-off bout, the same shortened stride turns the body the other way, so every bout undid the turn that the forward steps had made.
- Mirroring the turn during the bout (shortening the outer stride instead; `cl2_*`, `cl3_*`) did turn the body, but it tipped over backwards within 76-101 s in all three runs.
- Cancelling the turn command's stride inhibition while reversing (`cl4_*`, `cl5_learn`) tipped the robot over in its first bout (52-53 s). That inhibition also brakes the reversal, whose drive was tuned with it present ("41 fell head-on"); without it the reverse speed doubled (-12 vs -5.5 cm/s).
- Current version (`--reverse-steer 1`): AND cells `Q_l`/`Q_r` (turn command x reversal level, built like the existing yaw AND cells) give the outer stride the inner stride's inhibition while reversing. The body backs slowly and straight, keeping the brake but not the asymmetry. No falls in 4 runs of 800 s.
- That alone does not free a naive robot from a head-on wall: stride steering needs forward progress, and at the wall the body mostly steps in place between bouts. A replay of the recorded sensor stream through the probe shows the turn command active and one-sided (RS_l 13-16 Hz, RS_r 0). The project notes already record that this NAO's stride and hip-yaw steering are weak (about 1 deg/s).

**Closed loop: experience changes behaviour.** 3 m x 3 m walled room, start at the centre facing a wall; 800 s, no falls in any run. "Experienced" = the same network after the 16 scripted paired approaches in open loop (`nao_learn_openloop.py --export-after`), weights carried into the room, still learning.

| Robot (gated, homeostat off) | first entry into the reflex range (< 0.4 m) | time within 0.4 m of a wall |
|---|---|---|
| naive, frozen (`cl6_frozen`) | 49.6 s, first wall; never escapes | 745 of 800 s (93 %) |
| naive, learning (`cl6_learn`) | 49.6 s, first wall; never escapes | 745 of 800 s (93 %) |
| experienced, sonar map 0.45-1.5 m (`cl6_experienced`) | never: turns from the start, 1.0-1.5 m from walls | 0 of 800 s (0 %) |
| experienced, sonar map 0.45-0.9 m (`cl7_experienced09`, 646 s, cut short by a host crash) | 114.7 s, second wall, after avoiding the first (closest 0.63 m) and walking along it | 525 of 646 s (81 %) |

Summary: `target/qa/nao_learn/closedloop_summary.json`. One run per condition; the naive runs are near-identical because the learning robot's weights only change while the teacher is active.

- The naive robots learn while stuck at the wall, but cannot turn out faster. At the wall the learned and the innate turn are the same weak command. Learning pays off only before the wall, which is what the experienced robots show.
- In this 3 m room a 1.5 m learned range keeps the turn command on almost everywhere: the experienced robot avoids every wall but mostly rotates in place, because continuous stride shortening stalls forward progress.
- A 0.9 m map leaves room to walk. However, after turning ~90 deg along the first wall the robot met the next one nearly head-on, and 0.9 m of warning is too little for a ~1 deg/s turner.
- Free navigation of the room needs faster turning (body mechanics), or a range matched to the room, not more learning.

### Engine launch notes

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
