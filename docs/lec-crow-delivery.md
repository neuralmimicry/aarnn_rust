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
- **Correction (24 Nov): the stall is a low obstacle, not only the turn command.**
  - From about 50 s, both experienced robots (`cl6_experienced`, and the NPC-trained `test_walk_npc_trained`) are pinned against `target_ball`.
    - `target_ball` is a collidable sphere 0.225 m in diameter at (0.78, -0.645). Its top is below the chest sonars.
    - For the rest of the run the torso stays 0.18 m from the ball's centre, and the robot circles it (620-640 deg).
    - Forward speed is zero even while both sonars read clear (> 1.5 m, 23 % of the time).
  - So the 0 % rests on one early turn at the first wall. After that turn the experienced robots had no wall encounter at all (`nao_learn_closedloop.py report`: 0 encounters).
  - The naive robots are trapped at the wall itself (torso 0.38 m from the ball).
  - The NAO has no working sense for this obstacle. Its foot "bumpers" (`legacy-contact`, body-local +-y half-spaces of each sole, floor contacts included) read > 0.5 for 35-80 % of free walking. The real NAO's bumpers are at the front of the feet.
  - Needed: frontal bumpers that ignore the floor, and an innate touch-withdrawal reflex (the same reflex template as the sonar reflex, which could in turn teach the bottom camera). Alternatively, a room without the ball, to isolate the wall result.
- A 0.9 m map leaves room to walk. However, after turning ~90 deg along the first wall the robot met the next one nearly head-on, and 0.9 m of warning is too little for a ~1 deg/s turner.
- Free navigation of the room needs faster turning (body mechanics), or a range matched to the room, not more learning.

### drosophila_fafb never opened the MuJoCo display (fixed 24 Nov)

The command `./run_sim.sh --sim mujoco --robots drosophila_fafb=1` loads `network_drosophila_fafb.json`: the June FAFB import, 693 MB, 20,048 neurons, 915,051 synapses, with morphology, growth and delays at depth 5 (a 2.7 GB snapshot in memory). The launcher starts the bridges and MuJoCo only after the orchestrator reports the network placed on a node, and it waits 300 s by default. Placement never happened. There were four causes, each found by sampling stacks under gdb and reading memory use and logs:

1. Import, point-only anatomy reconstruction:
   - every route candidate was checked against every soma and every previously routed segment, and the per-neuron incident filter and "already routed" checks were quadratic as well;
   - fixed by indexing the bookkeeping (identical traversal and results; `morphology_contract.rs`) and by a connection budget (`NM_POINT_RECONSTRUCTION_MAX_CONNECTIONS`, default 50,000; 0 = no limit);
   - above the budget the import stays executable without derived anatomy, through the existing "reconstruction unavailable" path.
2. Import, runtime morphology (`Morphology::from_weights`):
   - the unique-geometry pass and the two micro-relaxation passes compared every synapse with every other (about 5x10^11 pairs);
   - fixed with grid broad phases that return the full-scan candidates in ascending order, so results are identical (test `synapse_grids_cover_every_pair_the_full_scan_accepts`);
   - the FAFB import now takes 38 s (`celegans_depth_probe`); before, it never finished.
3. The orchestrator loaded and re-aligned the full 2.7 GB snapshot (`load_network_config_or_snapshot`, and `load_io_contract_from_path`, which fully decoded a snapshot just to read its `net`):
   - about 19-40 GB and many minutes while holding the placement state, so the node could not register;
   - the I/O contract now comes from the metadata decoder (same `net`);
   - for snapshots over 256 MiB, when nodes preload from file, the orchestrator reads the configuration only;
   - `run_webot.sh` now defaults `NM_DISTRIBUTE_STARTUP_SNAPSHOT=auto` (it forced 1); remote compute still distributes.
4. Every run left a 2.7 GB snapshot copy in `logs/sim_cluster_*/startup-snapshots.*`:
   - `run_webot.sh` deletes it on exit, but `run_sim.sh` kills the cluster 1 s after TERM, before that runs;
   - `run_sim.sh` now removes the copies after shutdown.

Result (`target/qa/fafb_debug/run_sim6.log`, memory-capped scope):
- node ready in 80 s;
- orchestrator at 424 MB;
- placement confirmed ("Distributed across 1 nodes") about 2.5 min after launch;
- bridges ready, and the MuJoCo body ready with the brain bound.

Remaining limit: this network is too heavy to run interactively on one laptop. After placement the node used about 22 GB and a step took 114 s, so the fly does not move within minutes. For a live display, use a lighter configuration of this connectome (morphology and growth off, a lower depth) or the designed `drosophila_banc` circuit.

### NPC teacher experiments (started 24 Nov)

Direction from the user: pre-programmed NPCs as teachers, which learners experience and align with, and then apply what they learn to tasks the NPCs do not know.

**Infrastructure** (`adapters/mujoco/src/npc_actor.rs`):
- `NM_MUJOCO_NPC_ACTOR=script.json` adds a kinematic (mocap) NAO-sized orange actor to the learner's MuJoCo world. A sibling MJCF is loaded after the record's digest check; the pinned model is untouched.
- The actor has no contacts, and range and retinal rays see it.
- Scripts are keyframes in the world frame `[t, x, y, yaw]`, or in the learner frame `[t, distance, bearing, facing]` relative to the learner's chest and heading, so the NPC follows the learner.
- The actor's pose is traced as `npc_actor`. Tests: 4 in `npc_actor::tests`.
- Note: `run_sim.sh` runs `target/debug/aarnn-mujoco` (feature `viewer`).

**Perception check** (`scripts/qa/npc_perception_report.py`, `target/qa/npc_teacher/perception_frozen`; actor crossing ~1 m ahead):
- The chest sonars detect the NPC like any obstacle: 0.46-0.48 m reported at 0.50-0.58 m, in the +-20 deg beams.
- The cameras barely register it (6x8 ON/OFF retina, mean activity 0.001-0.006).
- So the NAO cannot yet tell a conspecific from a wall. Conspecific recognition is the missing sense for imitation proper.

**First experiment** (stand, train, then walk; `scripts/qa/npc_teacher_report.py`):
1. The learner stands (walk command silenced; the walking tonic is kept in `connectome_labels.npc_teacher`) while the NPC approaches it 16 times from +-20 deg, close enough to trigger the innate reflex, which teaches through the locus coeruleus gate.
2. The recorded senses are replayed through the plastic and frozen networks; the trained network is exported.
3. The walk command is restored and the robot is tested in the room without the NPC: a task the NPC never showed.

With world-frame approaches the reflex made the learner back away and turn (-47 deg over 480 s), so from the 4th approach the NPC missed its sonar beams (`train_npc`, not used). The learner-frame NPC fixes this (`train_npc_rel`).

**Result** (`target/qa/npc_teacher/analysis_rel/report.json`; threshold 8.87 Hz = half the frozen reflex peak):

| | left LT onset | left LT at 0.6-1.2 m | left sonar->LT weight | right LT onset | right LT at 0.6-1.2 m | right weight |
|---|---|---|---|---|---|---|
| approach 1 / 2 | 0.41 m | 0.4 Hz | 0.11 | 0.42 m | 0.4 Hz | 0.09 |
| approach 15 / 16, plastic | 1.25 m | 17.5 Hz | 1.18 (x13 of frozen) | 0.45 m | 3.0 Hz | 0.31 (x3.4) |
| approach 15 / 16, frozen replay | 0.42 m | 0.4 Hz | 0.09 | 0.40 m | 0.4 Hz | 0.09 |

- The left side learns anticipation from the NPC: LT now fires from 1.25 m, three times the reflex range.
- The right side strengthens but more slowly, because its innate reflex is weaker. Its onset has not moved yet.
- Frozen replay of the same senses stays flat, so the change is plasticity, not input.

**Transfer: walking alone, no NPC, 800 s** (`scripts/qa/nao_learn_closedloop.py`, `closedloop_summary_with_npc.json`):

| network | time within the 0.4 m reflex zone of a wall | first reflex | cells visited | fell |
|---|---|---|---|---|
| naive | 93.2 % | 49.6 s | 28 | no |
| trained by the scripted open-loop teacher | 0 % | never | 35 | no |
| trained by the NPC teacher | 0 % | never | 33 | no |

- On its first wall approach, the NPC-trained robot starts turning about 1.3 m from the wall. The naive robot starts at 0.47 m, then backs off and stays trapped.
- The NPC-trained robot's path is near-identical to that of the robot trained by scripted approaches: path length 94 vs 95 m, same places.
- The early turn is learned from an NPC walking up to the robot, and it carries over to a wall, which the NPC never showed.
- But from about 55 s, like the scripted-trained robot, it is pinned against the low `target_ball` (see the 24 Nov correction above). So the run shows one avoided wall, not sustained navigation.

Limits:
- One run per condition.
- The NAO cannot tell an NPC from a wall, so this is learning from the NPC's *presence*, not imitation of its behaviour.
- Imitation needs a conspecific sense; see the perception check above.
- The transfer test needs a sense for low obstacles (frontal bumpers) or a room without the ball.

Video: `target/showcase-videos/nao_learns_from_npc.mp4`.
- Segments: approach 1 (reflex only), approach 15 (anticipation from 1.25 m), walking alone (an early turn), then pinned on the ball.
- Pipeline: `target/qa/npc_teacher/video/make_cards.sh`.

### Realism and resilience (24 Nov)

Purpose of the per-body training (user, 24 Nov):
- Biology has thousands of neuron types and parameter sets; only a subset suits a given animal or device.
- The algorithms for stimulation flow, growth and pruning are the same for every body. The engineered complexity depth balances latency against measurable intelligence.
- Connectomes differ between species so that they connect to the anatomy of the nearest biologically equivalent body.
- Training per robot, per environment and per simulator is there to establish where each neuron sits in its body, and the length, growth and connections of its dendrites, axons and synapses.
- The designed circuits below use shared blocks (`aarnn_circuit`: hindbrain, oscillators, motor sequences) on each body's own senses. Their 3D layouts (`layout_designed_circuit.py`) are hand-placed starting anatomy, not yet learned. Morphology learned through growth and pruning is the next step.

Direction from the user:
- Make every body as true to the real robot or animal as possible: "make the fly look more like a fly"; make the NAO a SoftBank/Aldebaran NAO V6.
- Every robot should be available in every engine.
- Give robots and animals their real senses, including vision and hearing.
- Resource monitoring must pace itself rather than crash a process or the host.

**NAO V6 senses (every engine).** Sources:
- the Aldebaran NAO 6 technical documentation (sonar, cameras, contact sensors);
- the ros-naoqi NAO V50 URDF (camera, bumper and FSR frames, masses);
- the Webots R2025a `Nao.proto`, a V5 robot (sonar poses and the sonar sensor model).

- Cameras:
  - now on `HeadPitch_link`, so they move with HeadYaw/HeadPitch; v1 hung them level on the torso;
  - top camera at Head (0.05871, 0, 0.06364), pitched 1.2 deg;
  - bottom camera at (0.05071, 0, 0.01774), pitched 39.7 deg down, looking at the feet;
  - vertical field of view 43.7 deg (56.3 x 43.7 deg on a 4:3 retina).
- Sonar:
  - chest transducers at torso (0.0477, +-0.0416, 0.0509), aimed 25.8 deg out and 13.6 deg down;
  - a 60 deg cone sampled by 91 rays, listed in the record, so every engine casts the same rays;
  - reading = the nearest echo, V6 range 0.20-0.80 m (below 0.20 m it reads 0.20, presence only);
  - a ray whose incidence on the surface exceeds pi/8 is reflected away. This is the Webots "sonar" rule, and it means the floor never echoes.
  - One ping every 20 ms (sample and hold).
  - v1 used one ray per side, 20 deg out, reaching 2.55 m, the older NAO figure.
- Foot bumpers:
  - two ON/OFF switches at each toe (`region_m`: the toe shell from 10 mm above the sole);
  - standing never presses them; v1 used the left/right halves of each sole, which read the floor.
- Tests:
  - `nao_v6_toe_bumpers_and_sonar_cone_are_physical`: standing gives no bumper and no echo; the toe pressed on the 0.225 m target ball switches the bumper; the sonar sees that ball from 0.45 m.
  - The engines' measurements: MuJoCo (`echo_beyond_self`, `region_contact`), Webots (region; root-laser range proxy with the new minimum), Isaac (`senses.py`).
  - Unity (`NmNaoRobot.cs`) and Unreal (`NmNaoActor.cpp`): cone sonar and toe bumpers in their own channel layouts. Not compiled here.
- Kept from v1: link masses (5.305 kg, the V50 URDF sum; V6 is 5.48 kg), 8 phalanges per hand, and the 3-axis foot force instead of 4 FSRs per foot. Changing these needs an IO version.

**Touch withdrawal (NAO networks).**
- A press on either toe bumper of a foot drives a touch cell T of that side (lumbosacral dorsal horn, lamina V, in the 3D layout).
- T kicks the back-off bout BACK and the turn away (RS) of its side.
- The T -> RS gain (3x the sonar proximity drive) is the smallest at which RS latches through the spinal-brainstem path at S1. Measured: RS 17.7 Hz, BACK 19 Hz, reversal 18 Hz on a press.
- T is not a teacher by default (`--touch-teaches 0`): the learned cells are sonar distance cells.
- The NAO sonar reading is now taken from the body record. Defaults: proximity onset 0.7 m, learned distance map 0.45-0.75 m.

**Learning with the V6 senses** (`target/qa/nao_v6`; threshold: half the frozen reflex peak):

| training | left turn onset, approach 1 -> 16 | right | synapses | frozen replay |
|---|---|---|---|---|
| 16 scripted approaches (open loop) | 0.38 -> 0.66 m | 0.38 -> 0.58 m | | flat (0.38 m) |
| 16 NPC approaches (closed loop, standing learner) | 0.45 -> 0.75 m | 0.43 -> 0.75 m | x15 / x14 | flat |

- With the cone sonar the right side learns as well as the left. With the v1 single ray it barely learned.
- The onset now reaches the sonar's 0.8 m limit.
- Walking tests: see the results below.

**V6 walking tests, slow turning (stride steering only, 800 s each, `target/qa/nao_v6`):**

| network | first turn onset | time in the 0.4 m reflex zone | stalled |
|---|---|---|---|
| naive | 0.39 m | 90.4 % (trapped at the first wall from 33 s) | 94 % |
| 16 scripted approaches | 0.59 m | 88.7 % | 90 % |
| 16 NPC approaches | 0.59 m | 95.4 % | 96 % |

- With real sensing, learning still moves the turn earlier, but every robot is trapped.
- The ball confound is gone: the cone sonar sees the ball, and no bumper was pressed.
- The limit is turning, ~0.4 deg/s from stride shortening.

**Turning (24 Nov).**
- `scripts/qa/nao_turn_probe.py` drives the generated NAO with the walk network's own target pattern, through the servo path, without a brain.
- The NAO turns with its one hip yaw-pitch motor, and timing decides it:
  - in phase with the half-centres: ~1 deg/s;
  - lagged ~0.075 cycle: +-11 deg/s, nearly in place, no falls in 40 s;
  - later lags tipped the body on left turns.
- In the network:
  - yaw delay cells `YD_A`/`YD_B` (tau `--yaw-lag-ms`) drive the existing yaw AND cells (+-`--yaw-turn` 0.15 rad);
  - both are now on by default;
  - `--force-turn` latches a turn command for measurement.
- Closed loop, frozen V6 network, 50 s forced turns, no falls:

| yaw lag | left turn | right turn | drift |
|---|---|---|---|
| 150 ms | +3.8 deg/s | -7.2 deg/s | 0.03-0.48 m |
| 300 ms | +5.8 | -9.9 | 0.12-0.24 m |
| 400 ms (default) | +6.6 | | 0.17 m |
| 500 ms | +7.1 | | 0.12 m |
| 150 ms, no stride steering | +0.9 | | 0.54 m |

- That is 18-27x the stride-only turn (0.37 deg/s), mostly in place.
- In the closed loop, stride steering and hip yaw work together.

**Auditory orienting calibrated.**
- Open-loop probe of the walk network with the hearing group:
  - a sound louder on the left microphones (time difference 0.8) drives OR_l to 20 Hz and turns the head +0.53 rad;
  - the right mirror gives -0.54 rad;
  - silence gives 0.
- Bridge fix: `scripts/run_sim.sh` sized the TCP bridge from the profile table (NAO 250). It now uses the binding contract's channel count (`brain_sensory`), so a network with optional groups binds.

**V6b walking networks** (`target/qa/nao_v6b`; 268 inputs, 116 cells):
- touch, fast turning, hearing and auditory orienting;
- scripted open-loop training gives an onset of 0.66 / 0.61 m (frozen 0.38 m);
- NPC training with an audible NPC (footsteps): L 0.448 -> 0.75 m, R 0.445 -> 0.742 m.

**Corner trap of the trained walkers and its fixes** (800 s walks, `nao_learn_closedloop.py report`):

| network | reflex zone | stalled | wall encounters | 0.25 m cells | bumper pressed |
|---|---|---|---|---|---|
| V6b naive | 14.6 % | 46.9 % | 11 | 40 | 25.8 % |
| V6b, 16 scripted approaches | 87.8 % | 94.6 % | 2 | 10 | 0 % |
| V6b, NPC-trained | 85.1 % | 92.0 % | 2 | 11 | 0 % |
| V6c naive | 16.5 % | 48.0 % | 16 | 39 | 24.8 % |
| V6c, 16 scripted approaches | 62.2 % | 60.2 % | 15 | 36 | 0 % |

- With fast turning the naive robot explores, but both trained V6b robots were pinned in one corner for ~760 s. They rocked in place (yaw -50 to -69 deg) with an obstacle on each side.
- Cause: with obstacles on both sides, both learned turns (LT_l, LT_r) fire and the turn command flips between them.
- V6c (`--lt-cross 1.0`): each learned turn also inhibits the opposite turn command (commissural inhibition, on RS, which is not plastic). The trained robot now explores nearly as much as the naive one (36 vs 39 cells, 15 encounters).
- The trained robot never touches an obstacle (bumper 0 %), while the naive one presses a bumper 25 % of the time: the learned turn anticipates the contact.
- It still spends 230 s pinned once. There the learned turn from the farther obstacle on one side fought the innate near reflex from the nearer one.
- V6d (`--near-veto 2.0`): a near-sonar cell vetoes the turn toward its own side, whatever a learned turn asks (on RS: every synapse onto LT is plastic). Training and walk: `target/qa/nao_v6d`.

**Fall recovery.** User: "when a robot falls over it should also be trained to stand up / recover from a fall".
- `scripts/qa/nao_getup_probe.py` lays the generated NAO down on open floor and plays keyframe programs through the servo path.
  - A program is keyframes over ten joint groups (symmetric, or left/right for a roll).
  - Every program ends in a fixed tail: a balanced flat-foot squat (forward-kinematics solved), then a rise to StandInit.
  - Robustness: a program must work from the worst of three landings (tilt +-0.15, roll +-0.1, joint noise 0.05).
  - Definitions shared with the network: `scripts/nao_getup_program.py`.
- A whole-program evolution strategy had no slope: every failure ends on the floor with about the same score (80 generations at upright -0.18).
  - An evolved program that "stood" had pushed on the room's step, so the probe now trains on open floor only.
- **Staged (curriculum) search** (`--staged`): each stage has its own goal and is searched from where the best previous stage left the body:

| stage | goal (score) | result (worst of 3 starts) |
|---|---|---|
| kneel (3 keyframes) | hips up, knees down, head and chest off the floor, still, facing down | hands and feet, trunk 33 deg up, still |
| crouch (2 keyframes) | centre of mass over the feet, soles flat, knees off the floor | CoM over the feet 0.000-0.003 m, soles flat 1.0, trunk 40-50 deg |
| (fixed) squat, rise, StandInit | | upright 0.994, torso 0.32 m |
| roll (back -> front; 3 asymmetric keyframes) | chest down, still | chest axis -0.96 (prone) |

  - The crouch stage was the key: from hands and feet, the knee range (max 121 deg) does not allow sitting back on the heels.
  - Pushing the trunk back needs the shoulders opening (hands planted), not swinging the arms back, which collapsed the prop.

| program | starts that stand up | time |
|---|---|---|
| front (`target/qa/nao_getup/front_getup.json`) | 3/3 search starts + 8/8 others (tilt +-0.3, roll +-0.25, joint noise up to 0.15 rad) | 15.1 s |
| back = roll + front (`back_getup.json`) | 3/3 + 8/8 | 18.8 s |

- **In the network** (`build_nao_walk_network_json.py --getup front_getup.json --getup-back back_getup.json`):
  - gravity sense: accelerometer.x through a relay (vestibular nuclei) into FALL_front (chest-down, < 0.35) and FALL_back (chest-up, > 0.65);
  - lying chest-up plays the roll, which ends chest-down; that then plays the front get-up;
  - each program is a chain of keyframe cells (`aarnn_circuit.add_motor_sequence`), a fixed action pattern in the midbrain of the 3D layout:
    - a keyframe cell K (22 mV plateau) drives the joints' posture cells to its targets and is ended by its adaptation cell;
    - a latched handover starts the next: latch S, edge cell X = S and not K;
    - a trigger and a slow gate stop a running program restarting; a program that ends with the body still lying retries;
    - while it runs it silences the walk command and half-centres.
  - Durations are calibrated in the runtime (`measure_sequence`): front within 1.7 %, roll within 5 % of the found timing.
  - Pitfalls found on the way: idle graded cells leak ~0.035 each (12 of them held the gate on); a cell's own partial activation must never cut its starter or reset its latch (the next keyframe's adaptation does).
- Test harness: `NM_MUJOCO_START_POSE=front|back` (+ `NM_MUJOCO_START_Z`, `NM_MUJOCO_START_JOINTS`) starts the MuJoCo body lying.
  - The joints must be in StandInit: in the zero pose, the arms point into the floor face-down and threw the body onto its back.
- First closed-loop test (lock step, network only): the body did not follow.
  - Lying face-down, the chest sonar saw the floor and latched the back-off bout, the gait started, and the x-only fall sense was weak (accelerometer.x ~0.30 +- 0.06, not 0.255).
  - Keyframe drives sagged with the keyframe cell's own release.
  - Fixes:
    - the fall detectors and program gates silence the walk command, the half-centres and the back-off;
    - a two-axis otolith-like fall sense (x and z);
    - each keyframe drives the joints through a saturating motor relay M, with drives read from a curve measured in the runtime (`measure_drive_curve`).
- Closed loop after the fixes (`target/qa/nao_getup_net/net2_*`, 40 s):
  - **face-down start: standing at 22.5 s and held to 40 s** (torso 0.31 m, upright 0.99).
    - The first attempt reached the crouch (upright 0.96) and toppled backward onto its back.
    - The face-up sense then played the roll (face-down at 13 s) and the second get-up stood.
  - Face-up start: the roll worked (face-down at 4 s) and the get-up reached upright 0.71, then fell back. Later rolls only reached the side.
  - Crouch to squat is fragile in the adapter's servo path (it holds in the probe). A timed chain cannot notice that the body is tipping.
- This get-up is an engineered fixed action pattern found by an offline search. That is not how AARNN networks learn. Next (user, 24 Nov: AARNN networks learn by experience):
  - the found program becomes a teacher only, guiding the limbs in early trials;
  - body-state cells (vestibular, joint sense) tuned to the postures on the way have the only plastic synapses, onto the next movement's cells, gated by an LC cell when the body progresses toward upright;
  - then the teacher is switched off and the robot must right itself from unseen falls, each movement triggered by the sensed body state.
- **The learning test** (user, 24 Nov): stand up from a fall at any angle and position, on or near fixtures, adapting the movements. A fixed set of movements that works only on a flat, empty floor does not count.
  - Battery (`nao_getup_probe.py --battery`, room fixtures on): 8 places x 8 body rotations about the long axis (chest-down, 45 deg steps, on each side, chest-up).
  - The fixed programs choose front or back from the gravity sense.

| place | fixed programs stand up |
|---|---|
| open floor | 6/8 |
| next to the target ball | 6/8 |
| by a table leg | 6/8 |
| head to a wall | 3/8 |
| head on the step | 3/8 |
| side to a wall | 2/8 |
| feet to a wall | 0/8 |
| half under the table (0.24 m) | 0/8 |
| **total** | **26/64 (41 %)** |

  - Every side-lying start fails. Near walls and the step the roll fails. Under the table the robot would first have to crawl out.
  - This is the baseline the learned righting must beat. Its plan:
    - a repertoire of movement synergies (the found keyframes and their mirror images);
    - selection by body-state context through plastic synapses;
    - spontaneous exploration when nothing is selected;
    - learning gated by a progress signal (the gravity sense moving toward upright), trained over many closed-loop falls from varied starts.
- **Fall generator** (MuJoCo adapter, `NM_MUJOCO_REFALL_S`):
  - lays the body down again every period, uniform rotation about the long axis, tilt +-0.2 rad, one of `NM_MUJOCO_REFALL_PLACES`, seeded (`NM_MUJOCO_REFALL_SEED`);
  - the brain keeps running, so experience carries over from fall to fall;
  - each fall is logged on stderr.
- **Exploration and progress-gated learning** (`--getup-learn 1 --getup-select 1 --getup-explore 1`; `target/qa/nao_getup_explore`, 299 cells):
  - repertoire: the get-up, the roll and its mirror image; every step of each program can be entered through its own learning cell L;
  - context: body-state band cells (gravity sense and six joint senses);
  - selection: a righting-mode cell (lying on any side, or a program running) brings every L near threshold, and lateral inhibition lets the best-matching one win;
  - exploration: three slow oscillators with unrelated periods bias the L cells differently over time, so a different movement wins each try. On a synthetic side-lying body: get-up entry, squat step, mirrored roll, get-up, ...;
  - reward (innate vestibular values): P_up when the gravity bands step toward upright, P_ventral when they step toward chest-down while lying; both drive LC, which gates the band -> L plasticity;
  - each L stays active while its movement runs (so progress pairs with it), then is refractory.
- Pitfalls, all measured with open-loop replays of a recorded get-up:

| design | what happened | fix |
|---|---|---|
| progress = fast relay - slow relay of one sense | saturated at 20 Hz while standing still | band transitions (AND of excitation only) |
| band = relay(k) - relay(k+1), equal weights | every band whose lower relay was on stayed on. In the spatial layout, excitation delivers ~1.373x nominal and inhibition ~0.9x | inhibition x1.53 |
| graded bands in the AND | transitions never fired during a quick rise | binarised band copies, a trace held ~1-2 s |
| default learning rate | about 1 % change in 40 s | eta 0.3. Learning normalises each L's inputs (which state, not how much), hence competitive selection |
| point-cell layout | almost no learning | spatial layout (morphology and delays). The dendrite/bouton plasticity needs the anatomy |

  - The spatial layout changes every circuit calibrated with point cells (active dendrites); the Freenove hexapod drifted +82 deg spatial vs -4 deg flat.
- Closed-loop baseline on the test falls (fixed programs, fall every 25 s, seed 21, `target/qa/nao_getup_learn/test_fixed`): **3 of 12 recovered** within 25 s.
- The first closed-loop learning attempt froze, and open-loop replays of its sensors exposed more faults:

| fault | fix |
|---|---|
| MOVING summed the idle leak of 20 keyframe cells, so it read "moving" always and STUCK never fired | MOVING reads the motor relays M (silent at rest) |
| lateral inhibition from idle L cells held every L below threshold | inhibition through a thresholded relay per L |
| L held its keyframe on, and two movements (the roll and its mirror) latched together | L only starts a program (a one-shot pulse cut by the program's own gate) |
| a latched choice never ended, or two latched together (the hold is far stronger in the spatial layout) | window-ending adaptation 20, mutual inhibition 40 |
| the innate triggers started programs on top of explored ones | exploring, they are a preference onto that program's entry cell |

- Final structure:
  - learning cells only at program entry points (get-up start, crouch, squat; roll; mirrored roll);
  - each choice latches for a ~5 s eligibility window, so progress pairs with it;
  - one program at a time.
  - On a side-lying replay it explores get-up, mirrored roll and roll in turn.
  - Known imperfection: a choice made while another program runs cannot start anything, but can be credited with that program's progress.
- Online learning run (900 s, seed 21): `target/qa/nao_getup_explore/learn_run2`. The measure is the stand-up fraction per fall across the run against 3/12.
- Then hexapod, fly and fish righting.

**Hexapod v3 = the Freenove Big Hexapod Robot Kit for Raspberry Pi (FNK0052)** (user directive, 24 Nov).

Sources: Freenove's open-source code and tutorial (CC BY-NC-SA 3.0; only numbers are used, no code). Modelled (`freenove_hexapod_body`, `HEX_*` in `scripts/embodiment_legacy.py`):

| item | model | v2 |
|---|---|---|
| legs | coxa 33, femur 90, tibia 110 mm; radial from coxa mounts at (x fwd, y left) (+-76, +-55), (0, +-85) mm, corner legs 36 deg from the body axis | 56 mm links, all sideways |
| servos | 18 MG996R-class (stock photo; standard metal-gear): 0.92 N m, 6.16 rad/s, 0-180 deg about the 90 deg centre, full torque at 0.15 rad error | +-3.2-4 N m from the legacy proto |
| built pose | the kit's calibration pose (all servos at 90 deg: coxa radial, femur horizontal, tibia vertical) = joint zero | |
| mass | 2.12 kg: chassis 1.1 kg (plates, Pi, shield, 4x18650, six coxa servos), per leg 0.07 / 0.075 / 0.025 kg | 0.88 kg |
| sonar | HC-SR04 on the head: 15 deg cone, sonar specular loss, 0.02-3 m (the kit's software reads 3 m) | IR-style lookup to 1.2 m |
| camera | Raspberry Pi camera (OV5647) on the head, vertical field 41.4 deg | |
| IMU | MPU6050 on the shield (the contract's body accel/gyro) | |

- The kit has no foot switches, and its hobby servos report no angle. The contract's 6 foot-contact (now the tibia tip) and 18 joint-angle channels are simulated proprioception.
- The two SG90 head servos are not in the 18-output contract.
- Regeneration, the contract lane and the native tests pass.

Gait feasibility (`scripts/qa/hexapod_gait_probe.py`: per-leg IK tripod/wave gaits, the kit's 40 mm step, through the servo limits, no brain):
- the IK is exact (`scripts/hexapod_kinematics.py`, shared with the network builder);
- with a tall stance (coxa 0.10 m above the feet, feet 90 mm beyond the coxa link) the tripod walks 5.1 cm/s straight at 1 Hz; 0.3 rad stance rotation turns 32 deg/s;
- at 60 mm hip height it only reached 1-3 cm/s (femur stall).

**Walking network = the kit's own way of walking, on the kit's own senses** (`scripts/build_hexapod_cpg_network_json.py`).
- The stock kit walks open loop. Its software steps a fixed gait cycle through leg IK, and its servos report nothing back.
- The network is a central pattern generator on the shared blocks, using only the stock senses:
  - rhythm: one half-centre pair (`add_leg_oscillators`), ~0.9 Hz; phase A swings tripod lf/rm/lr, phase B rf/lm/rr;
  - lift: the half-centres raise the swinging tripod;
  - stride: two-cell lagged interneuron chains per side and phase (smooth S-shaped protraction and retraction);
  - posture: motor cells hold the IK stance; joint targets come from the probe's IK;
  - HC-SR04 obstacle turn through the shared hindbrain: relay, proximity cell P, bistable RS turn cells with the ARTR flip-flop choosing the side, and a turn bout that keeps its direction after the echo is lost (the HC-SR04 is blind beyond ~22 deg of incidence);
  - spontaneous run-and-turn bouts (`add_turn_bouts`) with back-off (BACK, the moonwalker-neuron analogue, reverses the stride through AND gates).
- The record's joint-angle and foot-contact channels are not used (the stock kit has neither).
- `--trim 0.2` (right stride x1.2, left x0.8) walks straight; 0.1 turned -52 deg and 0.3 +59 deg in 30 s.
- Closed loop (MuJoCo, lock step): ~4-4.5 cm/s on flat floor, tilt < 8 deg. In 120 s it walks to a wall, backs off and turns, and explores 12 cells of 0.25 m.

Fixes on the way:

| version | problem | fix |
|---|---|---|
| old Cruse reflex net | swing cells latched with femurs lifted; the robot lay on its belly; it needs sensors the kit lacks | CPG |
| CPG, NEAR cell | tonic -173 mV/ms against a 200 mV/ms sensor drive: never turned | the shared measured sonar relay and P cell |
| hindbrain v1 | turn gates at 0.27 release; an RS adaptation handed the turn to the other side mid-turn | AND gates 13/14; a separate turn bout |
| bout v1 | both RS on (left and right turns cancelled); 30 mV plateau never let go | RS mutual inhibition 12; 22 mV bout with a tonic in its band |
| single delay cell | exponential stride: a shove, then a stop; stalled on the habitat's ribbed mat (ribs 3.9 cm) | two-cell chain |
| no exploration | a front leg wedged on a slalom post outside the 15 deg cone: 96 s jammed | turn bouts + back-off |

- 3D layout rules (`layout_designed_circuit.py`, insect anatomy):
  - rhythm and premotor chains in the thoracic ganglia;
  - command in the central complex;
  - ARTR in the lateral accessory lobe flip-flop;
  - RS as suboesophageal descending neurons;
  - BACK as the moonwalker descending neurons.
- Still open:
  - the two SG90 head servos (pan/tilt for camera and sonar) are not in the 18-output contract;
  - joint-angle and foot channels could become optional groups;
  - a 1x1 camera cell only.

The legacy hand-made Webots `HexapodRobot.proto` is not updated. The native Webots adapter uses the record-generated Freenove model.

**Fly anatomy v4** (`scripts/embodiment_legacy.py`).
- The head (0.6 g) and abdomen (0.5 g) are now real bodies. Their moments cancel, so mass (10.3 g) and centre of mass are unchanged. v3 hung them as massless decoration.
- The compound-eye cameras sit on the head at the catalogue eye positions.
- Flight proof: in the air 13.8 of 15 s, attitude rms 2.1 deg (v3: 1.5 deg), unchanged hover.
- Real leg proportions from the flybody model (TuragaLab, Apache-2.0, CT-based; 50x scale) are built and kept off (`FLY_REAL_LEGS`):
  - attachment on the pro-, meso- and metathorax;
  - per-pair coxa, femur, tibia and tarsus lengths;
  - forelegs forward, hindlegs back.
  - With them the fly lost attitude: at 2 s with the v3 0.3 g links (pitch inertia x4, centre of mass 1.4 cm lower). With realistic 0.05 g links it lost attitude after 1.5-9 s, even with the ideal controller.
  - A realistic fly needs a flight-motor and control retune. The legs stay v3 until then.

**Other bodies.**
- Crow: each retina sits on its own side of the head, axis 26 deg outward. The fields overlap ~40 deg in front, the corvid binocular field. v1: both retinas at one point, looking ahead, saw the same image.
- C. elegans: the gentle-touch channels have receptive fields. `touch_front` is any of segments 12-23 (ALM/AVM, anterior half); `touch_rear` is any of segments 0-11 (PLM). v2 sensed only the nose and tail-tip segments. Names and count are unchanged.

**Optional sensor groups (user decision, 24 Nov).**
- A body carries all its real senses. Each sensor outside the original contract has a `group` (`vision`, `hearing`).
- A network binds exactly the named channels it was built with, in its own order (`Binding.sensor_names`, `selection()` in `aer.rs`; `scripts/mujoco_bindings.py`; `scripts/robot_aer.py`). Only optional-group channels may be left out.
- Preserved networks therefore keep their exact contract and are never resized:
  - `network_zebrafish.json` binds its 32 channels of the 228;
  - the BANC/FAFB connectomes bind their 418 of 424;
  - `network_nao.json` keeps 250.
- New or designed networks can also receive the new senses.
- An adapter caches a camera's retina per shape, because one eye can feed a 1x1 luminance cell and a 6x8 retina. This was a bug before groups.

| body | added (group) | channels |
|---|---|---|
| zebrafish | 6 x 8 ON/OFF retina per lateral eye on the existing eye cameras (vision) | 192 |
| zebrafish | inner ears (otolith organs), 100 and 300 Hz, nearly omnidirectional (hearing) | 4 |
| Drosophila (BANC, FAFB) | antennal Johnston's organs on the head, 150/300/600 Hz, strongly directional (hearing) | 6 |
| NAO V6 | 4 head microphones FL/FR/RL/RR (V5 head-chain positions) x 4 bands (250 Hz-8 kHz) + front/rear interaural time difference (hearing) | 18, enabled after the 24 Nov walking tests |

- NAO auditory pathway (`build_nao_walk_network_json.py --hearing 1`, used only if the body has the group):
  - cochlear-nucleus relays;
  - lateral superior olive level comparators;
  - medial superior olive time-difference cells;
  - superior-colliculus orienting cells that turn the head (HeadYaw) toward a sound.
  - Each stage has its anatomical place in the 3D layout.
- The crow's IO is a proposed projection (`spec/crow-io.proposed.json`). Its ears belong in that projection and are not added yet.

**Hearing (shared model).**
- `crates/aarnn-embodiment/src/acoustics.rs`:
  - free-field sources with spectra and on/off schedules;
  - spherical spreading;
  - receiver directivity that grows with frequency (head shadow, so high sounds give larger level differences between the ears);
  - incoherent summation;
  - interaural time difference.
- Sensor kind `legacy-acoustic` in MuJoCo, Webots and Isaac; scene `sound_sources`.
- The NPC actor makes footstep noise while it walks.
- Next, an IO version per body:
  - NAO V6: 4 omnidirectional head microphones FR/FL/RL/RR, 100 Hz-10 kHz;
  - the fly's antennal Johnston's organs, the fish's inner ear, the crow's ears;
  - each wired to its auditory relay cells (cochlear nucleus, superior olive).

**Self-protection under resource pressure** (`src/monitor.rs`).
- One cgroup-aware `ResourceSample` per process:
  - VmRSS and its peak;
  - the tightest `memory.max` on its own cgroup path (e.g. a `systemd-run -p MemoryMax=` scope);
  - MemAvailable, memory pressure (PSI), load, threads, free disk.
- Headroom and level: normal; warn (below 2x `NM_MEM_RESERVE_MB`, default 1 GB, or 10 % of the limit); critical (below the reserve).
- Fixed:
  - the CPU reading was always 0 (a new sysinfo `System` per call);
  - the GA stored KiB as MB and sized its thresholds from host RAM; it now uses the effective limit.
- Admission control:
  - nodes stream the snapshot metadata;
  - `estimate_runner_bytes` sums the dense matrices, plasticity and delay copies, decode and anatomy;
  - a node refuses a network that would not fit, with a clear error, instead of being OOM-killed.
  - The orchestrator no longer reads a large snapshot into a string.
  - Live: "node nao_0 admits ... (~67 MB estimated; normal (rss 22 MB, headroom 6030 MB, limit 6144 MB ...))".
- Pacing in every network worker (`monitor::Pacer`):
  - warn: steps slow down (1-50 ms) and structural growth pauses;
  - critical: steps hold;
  - critical for 30 s (`NM_PRESSURE_CRITICAL_GRACE_S`): the network pauses with its state kept.
- Heartbeats report RSS, limit, headroom, level, PSI, load, threads and free disk (proto fields 48-56). Capacity is scaled by the level (critical: 0), and placement uses the reported headroom.
- Disk:
  - snapshot autosaves and `runtime snapshot` writes check free disk (`NM_DISK_RESERVE_MB`, default 2 GB);
  - the UI picks the OpenGL renderer for captures or low VRAM (wgpu under Xvfb locked the GPU on 23 Nov).
- Launch scripts:
  - `scripts/run_sim.sh` and `run_webot.sh` run themselves inside a `systemd-run --user --scope` capped at `NM_MEMORY_MAX` (default 80 %) unless already capped (`NM_MEMORY_GUARD=0` to disable).
  - A runaway process is then killed inside the scope, not the desktop.
- Not done:
  - the placement planner still demands 8 bytes per neuron;
  - a critical node can still receive the planner's minimum share;
  - checkpoint-and-exit on critical memory is not implemented (the network pauses instead).

### Growth (neurogenesis at energy peaks), 24 Nov

User rule:
- growth stays off only for C. elegans, whose network failed with more than its 302 neurons;
- every other network grows until its own tests show that additional neurons do more harm than good.

- Until now the designed-circuit template froze structure for every body: growth, morphological growth, new edges, resonance gain and ambient energy were all off. No run had a single growth event. The original `network_nao.json` has growth on.
- The runtime's mechanism:
  - a cell at its firing-rate saturation (0.5) spawns a precursor;
  - the precursor is specified, migrates and differentiates;
  - it is placed at the highest-energy free location of an energy field built from local activity, morphology and ambient energy;
  - each spawn halves local energy;
  - admission is limited by area and membrane volume ("ConstrainedByMembrane").
- Measured:
  - NAO get-up network, replay of fall experience: +16 neurons in 10 s, placed in the vestibular nuclei (8), cerebellar proprioceptive relay (4), cochlear nucleus (2) and superior colliculus (2), where the activity peaks. Then constrained by membrane.
  - Freenove hexapod CPG with the template's newborn wiring: 38 -> 80 neurons in 15 s. Newborns joined the motor outputs at full strength (p_out 1.0, 0.05-0.25 each; w_out sum 1.2 -> 56), all outputs saturated in 3 s and the walk collapsed (0.05 m in 120 s instead of 1.07 m).
- Changes:
  - template: growth on except C. elegans;
  - newborn defaults for designed circuits: synapses x0.05 (`growth_new_synapse_scale`, new runtime parameter), designed cells keep their synapses (no migration), p_in/p_out 0.25;
  - new runtime parameter `growth_placement_mode` (high / low / mid);
  - the open-loop probe follows a growing network and dumps per-synapse route gains.
- Even with x0.05 newborns the hexapod's outputs still rose (0.50 -> ~0.9 mean, open loop, 30 s) in every placement mode. Two more causes, both fixed:
  1. **IO connectivity floor.** While growth is on, every step tops up each sensory channel with no hidden target (1 wire) and each output with too few hidden inputs ((hidden/12) clamped to 8-32 for invertebrates, 1 for humans), using random 0.1-0.4 wires to the nearest cells. That bootstraps random networks. In a designed circuit it gave the 18 servos 8 random wires each, and it silenced the swing half-centre (E_gait_sw 0 Hz) even when no neuron was born. New runtime parameter `growth_io_floor_enabled` (default true) gates the floor in both the point-cell and the morphology paths; designed circuits set it false.
  2. **Newborn cell type.** A newborn takes its type from its region's type distribution. Designed circuits have no `brain_regions`, so newborns got the template's default biology (spiking, synaptic gain 100). Driven by the 500 Hz posture cells they fired at ~250 Hz, and through x0.05 synapses still matched the designed motor drive. New runtime parameter `growth_newborn_type`: designed circuits name `Growth:newborn`, an immature graded interneuron (no tonic, release max 20 Hz, ~0.4 Hz at rest). It joins nearly silent and gains influence only through experience, as adult-born neurons do.
- Hexapod CPG, open loop, 30 s sonar ramp, after both fixes (`target/qa/growth_placement/hex_ol4.txt`):

  | placement | newborns | mean output (off 0.504) | \|output - off\| | E_gait_sw (off 10.3 Hz) | newborn to nearest designed cell |
  |---|---|---|---|---|---|
  | high | +15 | 0.533 | 0.049 | 10.0 | 0.107 |
  | low | +26 | 0.539 | 0.060 | 9.3 | 0.124 (11 in the suboesophageal descending neurons) |
  | mid | +23 | 0.540 | 0.061 | 10.0 | 0.063 |
  | high, old IO floor | +18 | 0.567 | 0.101 | 0.0 | 0.081 |

- Closed loop showed three more causes (all fixed):
  3. **Sensory attach.** In AARNN mode every newborn is attached to its nearest unused sensory channel with a 0.1-0.4 weight that `growth_new_synapse_scale` did not scale. Each newborn sat at -13 mV, at full release. It is now scaled.
  4. **Newborn scale.** 0.05 of the 0.05-0.25 draw is ~2 mV/ms per sensory synapse at full sensor value. Newborns saturated, every saturated cell spawned again, and growth cascaded to the neuron cap. The template now uses 0.0025 (~0.1 mV/ms per sensory synapse, ~0.004 mV/ms from a 20 Hz cell). Newborns rest at 0.1-0.6 Hz. Replayed on a recorded walk, the growing network tracks the growth-off one exactly.
  5. **Newborns never learned.** A cell past the end of a plastic-cell mask was frozen. `PlasticCellMask.newborns` now lets newborns learn (a critical period); the NAO learning builders set it.
- Results, after the fixes:
  - **Hexapod, closed-loop walk, 120 s** (`target/qa/growth_placement/walk_hex_*`, `hex_trace.py`):

    | growth | neurons at end | net displacement | path | explored cells |
    |---|---|---|---|---|
    | off | 38 | 1.10 m | 8.74 m | 12 |
    | neurogenesis, high | 116 (cap) | 1.07 m | 9.11 m | 12 |
    | neurogenesis, low | 116 (cap) | 1.03 m | 7.16 m | 11 |
    | neurogenesis, mid | 108 | 1.05 m | 8.44 m | 12 |
    | morphological growth only | 38 | 0.48 m | 2.21-2.60 m | 4-5 |

  - **NAO, open-loop obstacle learning, 12 paired episodes** (`target/qa/growth_placement/nao5`, look-ahead turn cell LT onset distance):

    | growth | neurons grown | first 3 | last 3 | mean |
    |---|---|---|---|---|
    | off | 0 | 0.38 m | 0.48 m | 0.425 m |
    | high | +100 | 0.38 m | 0.47 m | 0.421 m |
    | low | +100 | 0.38 m | 0.46 m | 0.420 m |
    | mid | +2 | 0.38 m | 0.48 m | 0.425 m |

  - **Placement:**
    - Growth: low places newborns farthest from the designed cells (0.12-0.19 vs 0.05-0.11) and reaches new regions (suboesophageal descending neurons). In the open-loop test it grows the most (+26 vs +15 high, +23 mid). In the NAO layout, mid placement found almost no admissible location (+2).
    - Stability: no placement harms the walk once newborns join silent.
    - Learning: no placement changes the NAO obstacle learning. Newborns join silent, and in these tests none gained influence. This is the open question for growth: newborns now learn, but nothing yet shows them contributing.
- **Morphological growth** (`morpho_growth_enabled`: neurite growth, contact synaptogenesis, synapse migration, soma movement) runs only in the closed-loop runtime. The probe skips it for point-cell networks. It stalled the hexapod walk in every variant:
  - contacts at the template's full strength 1.0 (~11 mV/ms each; 3,417 contacts in 120 s);
  - contacts at the silent strength with migration off (new runtime parameter `morpho_synapse_migration_enabled`; migration had moved ~560 premotor synapses onto other servos);
  - contacts no longer overwriting existing synapses. A contact between connected cells used to replace the designed weight; `apply_morpho_evolution_result` now keeps it.

  The remaining harm path is not found. The femurs settle at a uniform neutral posture, so the designed motor drive is lost. Designed circuits keep neurogenesis on and morphological growth off (`Circuit.morpho_growth`) until it preserves their function. Training-established neurite lengths and connections (user, 24 Nov) depend on this fix.

### Righting by experience: why M3 did not learn, and M4 (24 Nov)

Per-fall replay of the M3 run (`target/qa/resume_2026-11-24/tools/getup_episodes.py`, output `target/qa/nao_getup_explore/learn_run2_episodes.txt`). It stood 2 of 36 times:
- The roll programs never ran, although 17 falls landed on the back. The front get-up from fall 0 rolled NAO supine, and from then on only the front program ran: every ~8.3 s it re-entered and its gate cut every other start.
- No body-state band covered lying supine or on a side. The accelerometer.x code stopped at 0.54 (supine reads 0.75) and there was no accelerometer.y.
- Learning ran away. In 50 s the band synapses onto `L_getup_00` grew 43-fold. Every input of the plastic cell learned, so its control wiring (mode drive x5.5, oscillator bias x10, movement inhibition) hit the clamp and latched the entry for good. Frozen, the same entry only pulses.
- Progress fired without progress. The soft band edges left the next gravity band up at ~0.3 release while the body lay still, and the binarised copy counted that as reached, so `P_up` ran ~8 Hz and opened the learning gate the whole time.

M4 changes:
- Bands: accelerometer.x to 0.80; accelerometer.y with right side, centre and left side bands.
- Progress: a band counts only above 0.6 release (`--getup-progress-band-min`), held for 300 ms (`--getup-progress-hold-ms`).
- Runtime:
  - `PlasticCellMask.pre_hidden` restricts learning by presynaptic cell. Only body-state synapses (and newborns') learn; an entry cell's control wiring is fixed, as a Purkinje cell's climbing fibre is.
  - `aarnn_plasticity_w_max` caps a learning synapse at the circuit's scale. `nao_getup_learning.py make --w-max-mv`, default 0.3 mV/ms.
- Growth on (neurogenesis; newborns plastic); morphological growth off.

Open loop on the same sensor stream:
- the entries now alternate, including both rolls;
- the supine band fires lying on the back;
- `P_up` and `LC` stay at 0 while the body does not improve;
- learned weights stop at the cap.

Closed-loop test: `target/qa/nao_getup_m4/run1`, same fall generator.

### Learning through growth (night of 24-25 Nov)

User directive: "continue with the intent of the project, across all robots, all environments, all simulators. prove learning through growth."

**Status at 06:00, 25 Nov (all runs finished).** Open loop: frozen test after 24 approaches, growth on vs off, rng_seed replicates, newborns silenced afterwards. Closed loop: MuJoCo habitat, lock-step.

| body | open loop: what growth adds | newborns silenced | closed loop |
|---|---|---|---|
| Freenove hexapod | onset 0.96 -> 1.42-1.47 m, mid band 4.0 -> 15.5-16.4 Hz (3 seeds) | back to 0.94 m | time near obstacles 17.1 % off -> 9.2 % / 5.5 % in 2 of 3 growth seeds; the third wedged facing a wall. Cause found and fixed 25 Nov: the reversed stride reused the forward trim and cancelled left turns; with the fix the seed-2 walk is 6.1 % near against 47.5 %, but on the fixed gait growth off is also low (5.2 % against 3.0-6.2 %), so the earlier gap was mostly the broken escape. 600 s pairs on the fixed gait (26 Nov): growth 2.8 / 4.3 / 4.8 % near obstacles against 5.4 % off, a small gain in every seed; see "The seed-2 wedge" and "600 s pairs" |
| UGOT quadruped | onset 0.55 -> 0.86 / 1.41 / 0.98 m (3 seeds), no runaway | back to 0.58-0.59 m | one run, confined start: under 1 m from obstacles 24.6 % with growth (42 -> 17 -> 15 % per third) against 83.7 % off (ends parked at ~0.7 m) |
| Keenon T8 | onset 1.54 -> 1.62-1.66 m, far band +40-90 % (3 seeds) | back to 1.54 m | one run, 600 s: growth starts worse and improves (under 1 m: 29 -> 22 -> 10 % per third), growth off stays flat (19 -> 16 -> 19 %); total slightly worse (8.3 % vs 5.7 % under 0.66 m) |
| Drosophila (BANC walk) | far band 4.6 -> 9.5-9.6 Hz (3 seeds; unchanged by the protocol fix) | back to 4.6 Hz | not run |
| NAO, point cells | near 14.7 -> 18.7-19.0 Hz, onset 0.69 -> 0.73-0.74 m (3 seeds) | back to 14.7 Hz | first 195 s (before seed 2's fall): explored cells 23 / 21 with growth against 13 off and 12 frozen, on equal path (27 m); seed 7 over 600 s: 37 cells vs 19, stalled 35 % vs 66 % |
| zebrafish | far band 7.4 -> 10.9-11.7 Hz (3 seeds); the onset is at its 0.3 m sensing limit | back to 7.4 Hz | not run |
| C. elegans | growth off by design (connectome fails above 302) | - | - |

Bugs found and fixed tonight:
- the shared-ray protocol (front ray);
- the Keenon teacher threshold;
- inheritance on inhibition-coded cells (Keenon runaway);
- learner cells at the origin (fish newborns refused).

Not run tonight:
- Webots: its world is a 200 m bare floor with no obstacles, the controller build is 3 days old, and it needs the NVIDIA display (GPU lock-up risk unattended);
- Unity, Unreal and Isaac: same display constraint.

The test is growth on against growth off, on each body's own learning by experience, with the same seeds.
- Learning: teacher-gated anticipation. An innate near-obstacle reflex drives LC, the modulatory cell, and teaches a learning-turn cell LT. LT's silent synapses from distance-tuned sonar cells learn when an echo predicts the reflex.
- Measure: LT activity by obstacle distance over repeated approaches, and in the pause with no obstacle. A tonically active LT is not learning.
- Proof that growth carries learning: the gain over growth off, and its loss when the newborns are silenced afterwards (knockout).
- Harness: `scripts/qa/growth_learning_nao.py --body nao|hexapod` (learn, frozen test, knockout). Protocols: `nao_learn_openloop.py` and `hexapod_learn_openloop.py`.

Freenove hexapod learner (`build_hexapod_cpg_network_json.py --learn-groups 20`), insect anatomy:
- distance-tuned sonar cells CS (0.45-0.75 m) in the mushroom-body calyx;
- LT as a mushroom-body output neuron;
- LC as its dopaminergic teacher; the innate near turn (P_sonar, 0.40 m) drives both.
- Learning: the CS -> LT synapses grew x3.8 in 8 approaches. Over 24 approaches the onset moved from 0.35 m to 0.93 m (0.96 m frozen).

Mechanisms found and changed tonight (runtime parameters default to the old behaviour):

| problem | effect | change |
|---|---|---|
| matrix STDP with graded cells | in point-cell networks every LT input, the teacher's included, depressed to the floor; point-cell NAO and hexapod learned nothing (the spatial NAO learns through the dendritic-bouton path) | `aarnn_matrix_plasticity_rule` (oja for the point-cell learners) |
| every input of a plastic cell learned | the teacher synapse and control wiring changed | `PlasticCellMask.pre_hidden`: only named presynaptic classes learn (the climbing fibre stays fixed) |
| newborns came from the first saturated cell in index order | the gait CPG (built first, always saturated) fathered every newborn; none joined learning, and the hexapod learned slightly worse (0.91 vs 0.96 m) | the most active eligible cell spawns; `growth_parent_cells` sets a neurogenic niche, as adult neurogenesis is confined to niches (mushroom-body Kenyon cells in insects) |
| niche newborns never became neurons | `assess_growth` checked other areas before the parent's own; compact neighbouring nuclei overlap, so every newborn was "AmbiguousAreaOverlap" and refused | the parent's own area first; `NM_TRACE=1` logs `[growth-admit]` and `[growth-mature]` |
| newborns had no lineage | a newborn shared nothing of its parent's selectivity | `growth_inherit_sensory_inputs` (parent's sensory inputs x scale x jitter 0.5-1.5); `growth_new_input_synapse_scale` separate from the output scale (inputs before outputs mature) |
| newborns' own inputs learned too | niche newborns potentiated their sonar and newborn-newborn inputs to the cap and saturated; LT was held on at every distance, including with no obstacle | `PlasticCellMask.newborn_outputs` separate from `newborns`: inputs fixed (diverse inherited tunings), outputs onto LT plastic, so experience selects the newborns that predict the teacher |

Results so far (24 approaches; `target/qa/growth_learning`):
- NAO, spatial: growth off 0.38 -> 0.67 m (frozen test 0.68 m).
  - growth without a niche (+100, reticulospinal and lumbar): no change;
  - CS niche with learning inputs (+39): no change; newborn -> LT drive stayed negligible.
- NAO, point cells: no learning in any condition (STDP).
- Hexapod: growth off 0.35 -> 0.93 m.
  - niche with learning inputs (+78): LT saturated at every distance and in the pause - runaway, not learning;
  - fixed-input niche conditions: running.

**Result: learning through growth (25 Nov, 00:00-00:50).** Clonal newborns in the CS niche, with growth on (`aarnn_circuit.growth_niche_fields`):
- each newborn takes its parent's relay type (`growth_newborn_inherit_type`) and its sonar input jittered 0.5-1.5x, so each gets its own distance threshold;
- inputs are fixed, outputs silent and plastic;
- the bootstrap contact is skipped (at an inherited input scale of 1.0 the random 0.1-0.4 contact to a constant channel saturated every newborn).

Freenove hexapod, 24 approaches, frozen test after learning (`target/qa/growth_learning/hex`):

| condition | neurons | learned onset | LT 0.75-1.5 m | LT far | LT, no obstacle |
|---|---|---|---|---|---|
| growth off | 60 | 0.96 m | 4.0 Hz | 0.0 | 0.0 |
| growth, inherited input x1.0 | 60 + 78 | **1.46 m** | **15.9 Hz** | 0.4 | 0.0 |
| growth, inherited input x1.2 | 60 + 78 | **1.72 m** | **19.5 Hz** | 4.2 | 0.0 |
| growth x1.0, newborns silenced after learning | (60 + 78) | 0.94 m | 3.4 Hz | 0.0 | 0.0 |
| growth x1.0, plasticity frozen | 60 + 78 | 0.36 m (innate) | 0.07 Hz | 0.0 | 0.0 |

- Growth extends what is learned: anticipation reaches 1.5-1.7 m, against the designed sonar group's 0.45-0.75 m range.
- The gain is carried by the newborns: silencing them returns the robot to the growth-off level.
- It needs experience: with plasticity frozen, the newborns change nothing.
- The robot never turns with no obstacle in range.

Same harness on the other bodies (generic block `aarnn_circuit.add_learned_anticipation`, `--learn-groups N` in each builder, protocol `scripts/qa/body_learn_openloop.py`):

| body | neurons grown | with growth | growth off | newborns silenced |
|---|---|---|---|---|
| Drosophila walk (rays reach 8 cm) | +80 | LT far 9.5 / 9.55 / 9.57 Hz (3 growth seeds) | 4.6 Hz | 4.6 Hz; plasticity frozen 0.06 Hz |
| NAO, point cells (oja) | +73 / +73 / +74 | near 18.8 / 19.0 / 18.7 Hz, onset 0.74 / 0.74 / 0.73 m | 14.7 Hz, 0.69 m | 14.7 Hz, 0.69 m |
| UGOT quadruped | +80 | near 2.5 Hz | 0.35 Hz | 0.47 Hz (learner weak: inverting range cells uncalibrated) |
| zebrafish (rays reach 0.3 m) | +2 | no change | LT already active across the whole range | - |
| Keenon T8 | +80 | LT silent | LT silent | inverting cells never switched on (fixed) |

The inverting range cells (rays that code d / reach) are now calibrated like the legged explorer's proximity cells; the quadruped and Keenon reruns are running.

Rerun with direct proximity cells (`off3` / `growth3`, rng_seed 7), frozen test after learning:

| body | grown | LT test far / mid / near, growth | growth off | newborns silenced |
|---|---|---|---|---|
| UGOT quadruped | +80 | 0.08 / 0.58 / **13.5 Hz** | 0.00 / 0.05 / 5.2 Hz | 0.00 / 0.06 / 6.7 Hz |
| Keenon T8 | +80 | 0.00 / 0.01 / 0.18 Hz | 0.00 / 0.00 / 0.16 Hz | 0.16 Hz |

- Quadruped: growth more than doubles the learned near response, and silencing the newborns returns it to the growth-off level.
- Keenon: the learner still did not work. Two faults, both fixed on 25 Nov:
  - **Protocol, shared channels** (`body_learn_openloop.py`). The front ray belongs to both sides; whichever side was written last set it, so left approaches always read "far" on the front ray and lost that teacher. This affects every body with shared rays: quadruped, Keenon and Drosophila (4 of 6 rays shared). Their earlier runs had it in both conditions, so the growth comparisons stand, but the left turn was under-taught (quadruped LT_l inputs 0.11 against LT_r 0.72).
  - **Keenon teacher threshold** (`aarnn_circuit._range_cell`). The direct proximity cell ramps over ~0.3-0.5 m, and its switch-on drifts nearer as the coded threshold grows. At Keenon's 0.66 m teach distance (code 0.22) it gave 1.4 Hz at the protocol's nearest approach, so LC and LT barely fired. A two-point calibration (`_prox_inh_gain`: 1.0 at the quadruped's code 0.093, 1.5 at Keenon's 0.22) now puts ~5 Hz at the teach distance on both. (A single factor of 2.2 from a linear model switched the quadruped's teacher on at 1 m: rejected.)
- Reruns with both fixes (`v4off_s{7,4,5}`, `v4growth_s{7,4,5}`) are running for both bodies, plus a Drosophila rerun with the protocol fix.

UGOT quadruped with both fixes (frozen test after 24 approaches; the switch-on correction is 1.001 at its teach code, so only the protocol fix changes its network inputs):

| rng_seed | growth: onset / LT far, mid, near / no obstacle | growth off | newborns silenced |
|---|---|---|---|
| 7 | **0.86 m** / 0.9, 6.7, 19.6 Hz / 0.04 Hz | 0.55 m / 0.02, 0.37, 11.9 Hz / 0.00 | 0.59 m / 0.03, 0.57, 14.3 Hz |
| 4 | **1.41 m** / 10.0, 18.1, 20.0 Hz / 0.39 Hz | 0.55 m (same) | 0.59 m |
| 5 | **0.98 m** / 1.0, 9.2, 19.8 Hz / 0.03 Hz | 0.55 m (same) | 0.58 m |

- In every seed, growth (+80 newborns in the CS niche) extends the learned turn from 0.55 m to 0.86-1.41 m, and silencing the newborns returns it to 0.58-0.59 m.
- No runaway: LT stays at 0.03-0.39 Hz with no obstacle. Seed 4 generalises furthest (10 Hz in the far band).
- With the protocol fixed, the two sides are now balanced (sensory inputs to LT_l / LT_r 0.80 / 0.79 growth off, against 0.11 / 0.72 before).

Keenon T8 with both fixes (rng_seed 7 / 4 / 5, identical to two decimals across seeds):

| condition | onset, learning | onset, frozen test | LT far / mid / near | LT, no obstacle |
|---|---|---|---|---|
| growth off | 0.97 -> 1.55 m | 1.54 m | 2.8 / 17.8 / 19.8 Hz | 0.03 Hz |
| growth (+80), inherited input x1.0 | 1.33 -> 1.86 m | 1.86 m | 19.8 / 20.0 / 20.0 Hz | **14.5 Hz** |
| same, newborns silenced | | 1.54 m | 2.8 / 17.8 / 19.8 Hz | 0.03 Hz |

- The Keenon now learns: growth off moves the turn from 0.97 to 1.55 m in 24 approaches.
- With growth, LT is tonic (14.5 Hz with no obstacle): a runaway, not learning, even though the onset reads 1.86 m (the protocol's starting distance).
- Cause: 21 of 80 newborns fire with no obstacle (their CS parents: 0.04 Hz). A newborn inherits its parent's sensory input x 0.5-1.5, which suits excitatory relays. On these inhibition-coded proximity cells it weakens the inhibition, and newborns with 0.52-0.76 x the parent's input fire at 4-12 Hz at every distance; those at >= 0.95 x stay silent. Their outputs then learn to the cap and hold LT on.
- Rule: a clone must keep its parent's silence with nothing near, so on an inhibition-coded cell its inherited input must never be weaker than the parent's. Here: `growth_new_input_synapse_scale` 2.0 (inheritance 1.0-3.0 x; the newborns tune at or nearer than their parents). Running as `v4growthB_s{7,4,5}`. The UGOT quadruped's earlier growth3 run had no such runaway at x1.0 (no-obstacle LT 0.00 Hz): its cells sit further from switch-on at far.

With inheritance x2.0 (`v4growthB`, frozen test):

| rng_seed | growth: onset / LT far, mid / no obstacle | growth off | newborns silenced |
|---|---|---|---|
| 7 | **1.65 m** / 4.0, 19.3 Hz / 0.04 Hz | 1.54 m / 2.8, 17.8 Hz / 0.03 Hz | 1.54 m / 2.8, 17.8 Hz |
| 4 | **1.62 m** / 4.5, 18.8 Hz / 0.05 Hz | 1.54 m | 1.54 m |
| 5 | **1.66 m** / 5.3, 19.5 Hz / 0.05 Hz | 1.54 m | 1.54 m |

- No runaway, and a consistent gain carried by the newborns in every seed: onset +0.08-0.12 m and far-band response +40-90 %, back to the growth-off level when they are silenced.
- The gain is smaller than on the quadruped. The Keenon's growth-off learner already fills its mid band (17.8 of 20 Hz), and its newborns now tune only at or nearer than their parents.

Closed loop, Freenove hexapod in the MuJoCo habitat, 600 s, growth off (`target/qa/growth_learning/hex/walk`, `scripts/qa/hexapod_near_score.py`):

| network | time < 0.40 m from an obstacle | entries into that range | explored cells (0.25 m) |
|---|---|---|---|
| frozen | 11.7 % | 67 | 36 |
| learning | 6.6 % | 31 | 45 |

Replicates (growth seeds via the snapshot `rng_seed`; the probe's own seed does not reach growth): growth tests at 1.46 / 1.42 / 1.47 m and mid-band 15.9 / 15.5 / 16.4 Hz. Growth off gives 0.96 m and 4.0 Hz in every seed; newborns silenced give 0.94 m.

Closed loop with growth (same habitat and start, 600 s, 138 neurons at the end), share of time within 0.40 m of an obstacle:

| network | 0-200 s | 200-400 s | 400-600 s | total | entries |
|---|---|---|---|---|---|
| frozen | 13.7 % | 9.7 % | 11.7 % | 11.7 % | 67 |
| learning, growth off | 9.3 % | 6.5 % | 3.8 % | 6.6 % | 31 |
| learning + growth (x1.0) | 28.9 % | 2.2 % | 0.4 % | 10.5 % | 48 |
| learning + growth (x1.2) | 7.0 % | 64.1 % | 0.3 % | 23.8 % | 100 |

- By the last third, the growing learners almost never come near an obstacle (0.3-0.4 % against 3.8 %).
- But each had a stretch stuck facing an obstacle at 0.3-0.4 m: x1.2 from 200 to 380 s, x1.0 around 60-80 s.
- Likely cause: the learned turn (strong and sustained with growth) and the innate one drive both turn cells, so the winner-take-all fails, both turn commands cancel and the stride is inhibited.
- Next test: the learner with RS mutual inhibition 24 (`--rs-mutual`).

With RS mutual inhibition 24 (`walk_m24_*`, 25 Nov 01:49-02:25, rng_seed 7; open-loop learning unchanged):

| network | 0-200 s | 200-400 s | 400-600 s | total | entries | longest stretch < 0.40 m | explored cells |
|---|---|---|---|---|---|---|---|
| frozen (no plasticity, no growth) | 21.5 % | 9.6 % | 2.8 % | 11.3 % | 74 | 3.2 s | 49 |
| learning, growth off | 24.0 % | 1.2 % | 7.1 % | 10.8 % | 68 | 5.0 s | 49 |
| learning + growth | 11.5 % | 6.5 % | **0.7 %** | **6.2 %** | **56** | 6.3 s | 28 |

- The stuck episodes are gone: the longest stretch near an obstacle with growth falls from 23-32 s (without mutual inhibition) to 6.3 s. The winner-take-all diagnosis holds.
- Growth has the least time near obstacles overall and in the last third, and the shortest stays per entry (0.66 s against 0.95 s off and 0.92 s frozen).
- Caveats:
  - the frozen network's near time also falls over the run (21.5 -> 2.8 %), so part of the late-window decline is where the walk takes the robot, not learning;
  - the growing learner covered fewer cells (28 against 49): it turns away earlier and circles in open floor;
  - one seed per condition. Seeds 2 and 3 for growth and growth off are running (`walk_m24_{growth,off}_s{2,3}`).

Seed replicates (25 Nov 04:05-05:05). All four hit the 1 h wall-clock limit on the overloaded host: growth reached ~281 s, growth off ~429 s. So the comparison below covers the first 280 s:

| network | time < 0.40 m, 0-280 s | 0-140 s | 140-280 s | entries | longest stretch | explored cells |
|---|---|---|---|---|---|---|
| growth off (rng_seed 7, 2 and 3) | 17.1 % | 30.8 % | 3.4 % | 48 | 3.2 s | 26 |
| frozen | 18.4 % | 27.2 % | 9.5 % | 51 | 3.2 s | 18 |
| growth, rng_seed 7 | 9.2 % | 15.1 % | 3.4 % | 48 | 4.7 s | 12 |
| growth, rng_seed 3 | **5.5 %** | 7.6 % | 3.4 % | 24 | 2.2 s | 34 |
| growth, rng_seed 2 | 49.0 % | 46.4 % | 51.6 % | 123 | 9.0 s | 10 |

- Growth off is deterministic: rng_seed reaches only growth, and the three growth-off runs are bit-identical. The baseline is therefore one trajectory.
- Two of three growth seeds halve or better the early time near obstacles (9.2 % and 5.5 % against 17.1 %).
- Seed 2 wedged at 80 s: facing an obstacle at 0.39-0.42 m, it stepped in place for the rest of the run, motor rates near neutral, yaw wobbling 0-16 degrees, position fixed.
  - Replaying a held 0.40 m echo through the learned network gives a clean decision (RS_r 18 Hz, RS_l 0, the bout held), so this is not the two turn cells cancelling. The turn is commanded but makes no headway.
  - Likely a leg caught on the obstacle, the jam the explore bouts were added for. Not resolved tonight. (Wrong: see the next paragraph.)

**The seed-2 wedge: cause and fix (25 Nov morning).** The obstacle was the arena wall (x = 1.3 m), met head-on. The hexapod's two sonar channels are one HC-SR04, so both read the same distance (0.32-0.42 m) and only the ARTR picks the side.

What happened:
- Replaying the walk's own sensor stream through its network gives a clean decision: RS_r, a left turn, at ~15 Hz. BACK stays on for the whole bout as well, because the bout cell holds it and P/LT keep re-kicking the bout (growth made LT stronger).
- Measured yaw rate in each replayed state:

  | state | seed 2 | growth off |
  |---|---|---|
  | left turn + BACK | +0.6 deg/s | +2.2 deg/s |
  | right turn + BACK | -3.4 deg/s | -14.8 deg/s |

- The body is symmetric. The programmed gait (`scripts/qa/hexapod_gait_probe.py`) turns +-22.9 deg/s in place, and +22.9 / -17.0 deg/s while reversing.
- The fault was in the network. The reversed stride (the Z cells) reused the forward stride's trim (right x1.2, left x0.8), which corrects the forward gait's drift. In reverse that trim yaws the body right, so it cancelled every left turn made while backing.
- A left-chosen escape from a wall therefore never turned. The hexapod rocked at the back-off threshold for 200 s.

Two fixes, as flags in `build_hexapod_cpg_network_json.py` (default off):
- `--back-untrim 1`: the reversed stride ignores the trim.
- `--back-onset-ms 1500`: BACK only at a bout's onset, through slow feed-forward inhibition from a new cell BACK_I. The hexapod reverses once and then turns in place, as worms and flies do.

Same network and seed, 180 s, rebuilt by `target/qa/resume_2026-11-24/tools/hex_wedge_variant.py`. The rebuilt control reproduces the original walk exactly. Times near obstacles are per 60 s window:

| variant | near the obstacle (< 0.40 m) | per 60 s | longest stretch |
|---|---|---|---|
| control (as walked) | 47.5 % | 19 / 71 / 55 % | 9.0 s, wedged from 54 s |
| reversed stride untrimmed | **6.1 %** | 9.9 / 4.6 / 2.6 % | 2.2 s |
| BACK at onset only | 7.1 % | 14.5 / 6.7 / 0.0 % | 2.8 s |
| both | 8.4 % | 24.4 / 0.9 / 0.0 % | 3.0 s |

- All three escape the wall by 80-90 s, and the time near obstacles keeps falling across the run.
Replicates over the first 300 s (`target/qa/growth_learning/hex/wedge/walk_{untrim,both}_{s7,s3,off}`), time near the obstacle per 100 s window:

| network | as walked (trim in reverse) | untrimmed reverse | untrimmed + BACK at onset |
|---|---|---|---|
| growth, seed 7 | 16.2 / 6.8 / 12.0 % (mean 11.7) | 0.0 / 4.2 / 4.8 % (3.0) | 15.4 / 3.9 / 0.1 % (6.5) |
| growth, seed 3 | 6.5 / 6.5 / 3.0 % (5.3) | 5.2 / 6.7 / 6.6 % (6.2) | 13.2 / 0.0 / 0.0 % (4.4) |
| growth off | 37.9 / 10.1 / 2.4 % (16.8) | 4.7 / 5.8 / 5.2 % (5.2) | - |
| growth, seed 2 (180 s, per 60 s) | 19 / 71 / 55 % (wedged) | 9.9 / 4.6 / 2.6 % | 24.4 / 0.9 / 0.0 % |

- With the reversed stride untrimmed (now the builder default), no run wedges. Longest stretch near an obstacle is 1.6-3.4 s, against up to 9 s before.
- **The closed-loop growth advantage reported above does not survive the fix.** With untrim, growth off is at 5.2 % and the growth seeds at 3.0-6.2 %. Much of the earlier gap (17.1 % off against 9.2 / 5.5 %) came from how often each run picked the broken left escape.
- "BACK at onset" gives the clearest within-run learning curve: the last 100 s is 0.0-0.1 % in both growth seeds. It has not been run on growth off.
- Needed: 600 s growth-on vs growth-off pairs, 3 seeds, all on the fixed gait.

600 s pairs on the fixed gait (reversed stride untrimmed, no BACK_I; learning on; `target/qa/growth_learning/hex/pairs600`, 26 Nov 00:11-00:55; `hexapod_near_score.py --window-s 200`):

| network | 0-200 s | 200-400 s | 400-600 s | total | entries | longest stretch | explored cells (0.25 m) |
|---|---|---|---|---|---|---|---|
| growth off | 5.2 % | 7.7 % | 3.3 % | 5.4 % | 34 | 3.0 s | 51 |
| growth, rng_seed 7 | 2.1 % | 3.7 % | 2.5 % | **2.8 %** | 33 | 3.3 s | 40 |
| growth, rng_seed 2 | 7.3 % | 0.9 % | 6.3 % | 4.8 % | 33 | 2.9 s | 60 |
| growth, rng_seed 3 | 5.9 % | 4.3 % | 2.5 % | 4.3 % | 41 | 2.7 s | 52 |

- No run wedges in 600 s. Seed 2, which wedged before the fix, explores the most ground (60 cells).
- All three growth seeds spend less time near obstacles than growth off (2.8-4.8 % against 5.4 %; mean 4.0 %). In the last third, two of the three are lower (2.5 % against 3.3 %).
- The gain is small and does not grow steadily over the run. Growth off is a single trajectory (it is deterministic), and windows of the same run vary by 2-5 points. So this is a consistent direction, not a clear closed-loop proof.
- On the hexapod the open-loop growth result (onset 0.96 -> 1.42-1.47 m, removed by silencing the newborns) is the strong evidence. The closed-loop gain is modest.

NAO, closed loop in the walled MuJoCo room, 600 s, point-cell learner (`target/qa/growth_learning/nao_cl`, `nao_learn_closedloop.py report`):

| network | wall encounters | explored cells (0.25 m) | stalled | bumper pressed | reflex zone |
|---|---|---|---|---|---|
| learning, growth off | 2 | 19 | 66.1 % | 38.0 % | 7.8 % |
| learning + growth (+73 clonal newborns) | 11 | 37 | 35.2 % | 22.6 % | 7.9 % |

- The growth-off run spent 47 s inside the reflex range at its second encounter and stalled for most of the run.
- With growth, most encounters ended within about a second inside the reflex range (exceptions 31 s and 9.8 s), with turn onsets at 0.48-0.72 m.
- One run each (closed loop is chaotic): supporting evidence, not a proof.

Replicate and control (25 Nov, 01:50-02:43):

| network | wall encounters | explored cells | stalled | bumper pressed | reflex zone | fell |
|---|---|---|---|---|---|---|
| frozen (no plasticity, no growth) | 13 | 30 | 48.7 % | 0.0 % | **57.2 %** | no |
| learning + growth, rng_seed 2 | 5 | 26 | 65.9 % | 0.0 % | 5.9 % | **yes, at 198 s** |

- Frozen spent 57 % of the run inside the reflex zone, with stays of 169 s and 171 s. Both learners (growth on or off) kept that under 8 %, so learning, not the reflex alone, clears the walls.
- The rng_seed-2 growth run fell backwards while backing off a wall: pitch went from -4 to -67 degrees in one second while the robot moved back, with no bumper contact. This is the back-off gait, not the learner; neither seed-7 run fell. With the robot down from 198 s, its stalled share counts the fall.
- So in closed loop on the NAO, growth is not yet shown to beat growth off: seed 7 favours growth, seed 2 is lost to the fall. Growth off at rng_seed 2 is running for a paired comparison.
- Growth off at rng_seed 2 is bit-identical to rng_seed 7 (growth off is deterministic). Before seed 2's fall, both growth seeds explored more on the same path:

  | 0-195 s | path | explored cells |
  |---|---|---|
  | growth, rng_seed 7 | 27.4 m | 23 |
  | growth, rng_seed 2 | 27.4 m | 21 |
  | growth off | 27.3 m | 13 |
  | frozen | 25.0 m | 12 |

  Growth explores ~1.7x the room in both seeds. Over the full run only seed 7 counts (37 against 19 cells).
- Falls are a standing NAO gait risk, not a growth effect. In 39 earlier NAO walk traces (get-up runs excluded), three fell without growth: `nao_learn/cl2_frozen` at 76 s, `cl2_learn` at 78 s and `cl3_learn09` at 102 s, the first a frozen network.

Keenon T8, closed loop in its MuJoCo room, 600 s, rng_seed 7 (`target/qa/growth_learning/keenon/walk`, `tools/ray_near_score.py`, rays proximity_0/1/7):

| network | under 0.66 m (teach), per 200 s | total | under 1.0 m, per 200 s | total | entries (0.66 m) | explored cells | path |
|---|---|---|---|---|---|---|---|
| learning, growth off | 4.8 / 5.7 / 6.6 % | 5.7 % | 19.4 / 15.9 / 19.0 % | 18.1 % | 60 | 298 | 190 m |
| learning + growth (x2.0 inheritance) | 11.2 / 8.7 / **5.1 %** | 8.3 % | 29.2 / 21.8 / **10.1 %** | 20.3 % | 102 | 286 | 167 m |

- With growth the robot improves over the run, as the hexapod did; without growth it does not. Overall, growth is slightly worse.
- One run each, so this is a learning-curve signature, not a proof.

UGOT quadruped, closed loop, 600 s, rng_seed 7 (`target/qa/growth_learning/quad/walk`):

| network | nearest ray, median | under 1 m, per 200 s | under 1 m, total | under 0.28 m (reflex) | explored cells | path |
|---|---|---|---|---|---|---|
| learning, growth off | 0.69 m | 51 / 100 / 100 % | 83.7 % | 0 % | 8 | 48 m |
| learning + growth | **1.03 m** | 42 / **17 / 15 %** | **24.6 %** | 0 % | 6 | 45 m |

- Both runs stay inside a ~0.4 x 0.5 m patch of the start area and never reach the 0.28 m reflex, so this tests the learned turn alone, in a confined start.
- Without growth the robot ends parked ~0.7 m from an obstacle for the last 400 s. With growth it keeps clear and improves over the run.
- One run each.

Zebrafish, why growth did nothing (+2 newborns):
- The builders give each designed cell its position, but the generic learning block placed LC, LT, TN and CS at the origin. A newborn is born next to its parent, so every fish newborn landed at the origin of a laid-out body, inside two overlapping real nuclei: 37 of 39 were refused (`AmbiguousAreaOverlap`).
- Fix: `layout_designed_circuit.zebrafish` places the learner in the adult telencephalon:
  - CS in the dorsal pallium Dp (the olfactory-cortex homolog; the fish's learning channels are olfactory, and the adult zebrafish pallium is a neurogenic niche);
  - LT in Dm (the amygdala homolog);
  - the TN teacher relays in the olfactory bulb;
  - LC in the locus coeruleus.

  `aarnn_circuit.apply_learner` applies the profile's rules to learning cells still at the origin (`_place_learner_cells`). Now 39 of 39 newborns are admitted (31 within Dp, 6 expanding it, 2 attached to the nearest area).
- The fish learner is also at ceiling from the first approach: its CS -> LT weights reach the cap (10 x 0.136) in episode 0, and its rays reach only 0.3 m, so there is no range left for growth to extend. Reruns with the placed learner (`v2off` / `v2growth`, rng_seed 7 / 4 / 5) are running.

Zebrafish with the placed learner (frozen test after 24 approaches):

| condition | neurons grown | LT far / mid / near | onset | no obstacle |
|---|---|---|---|---|
| growth, rng_seed 7 | +80 (was +2) | **11.1** / 20.0 / 20.0 Hz | 0.27 m | 0.00 Hz |
| growth, rng_seed 5 | +80 | **10.9** / 20.0 / 20.0 Hz | 0.27 m | 0.00 Hz |
| growth, rng_seed 4 | +80 | **11.7** / 20.0 / 20.0 Hz | 0.27 m | 0.00 Hz |
| growth off (all seeds) | 0 | 7.4 / 19.9 / 20.0 Hz | 0.26 m | 0.00 Hz |
| growth, newborns silenced | (+80) | 7.4 / 19.9 / 20.0 Hz | 0.26 m | 0.00 Hz |

- Once newborns can be admitted, growth raises the learned far-band response by ~48 %, and silencing them removes the gain.
- The onset cannot move: the fish's rays reach only 0.3 m, so the learned turn already starts at the edge of its senses.
- The Drosophila, quadruped and Keenon learners are also unplaced (and the quadruped, fly and Keenon base circuits too). With everything at the origin, newborns fall outside every area and attach to the nearest, so growth was not blocked there. Placing them is still to do.

Righting learner M4b (state bands, held progress, winner-gated starts, innate gravity veto) did not stand in 13 closed-loop falls. The front program still runs while supine, and LC never fired. Shelved tonight; see the M3/M4 section.

### Learned foraging and recharging (design, 25 Nov)

The goal, in the user's words (25 Nov):
- The biomimetic bodies (C. elegans, Drosophila, zebrafish, crow) use their senses and needs to seek food.
- The machines use their sensors, actuators and needs to seek power.
- Every body explores its environment naturally.
- The seeking is learned:
  - a newborn feeds when food is presented but does not seek it;
  - a robot is given power before it seeks it;
  - experience, survival and interaction with others teach the needs through action. Nothing links a need to an approach by wiring.

What exists today (survey, 25 Nov):
- Nothing seeks food or power.
- Only the crow has need channels (`hunger`, `thirst`, `fatigue`, `energy_reserve`, `beak_taste_*`, set at `adapters/mujoco/src/world.rs:614-621`). Its energy drains with mechanical power (`world.rs:1004-1017`) and never refills.
- Only C. elegans has a sensed field:
  - `scene.stimuli` / `field_value` in `crates/aarnn-embodiment/src/aer.rs:141-166, 248-306`;
  - filled by `scripts/embodiment_scene.py:117-160`;
  - read at `world.rs:734-738`.
- Food objects are in the C. elegans, crow, fly and fish scenes but are inert; the scene notes say "scenery never supplies … food energy".
- Every profile already has a `light_beacon`.

Design. Only the physiology and a feeding reflex are innate; the seeking is learned:

| part | innate (body / world) | learned (network, plastic) |
|---|---|---|
| energy | a store that drains with basal use plus mechanical power; at 0 the body stops (as the crow) | - |
| feeding | food at the mouth (taste) or contact with a dock: the body stops and energy flows in while it stays, capped at full | - |
| interoception | channel `energy_reserve` (battery state of charge on robots, hunger on animals) and a `consume` channel (taste / charge current) | - |
| reward | an energy-gain transient cell (fast minus slow copy of `energy_reserve`) drives the modulator (LC/dopamine-like) | - |
| cues | food odour (a field around food items); dock beacon (light, IR or sound) | which cue predicts energy, and in which internal state |
| exploration | the existing run-and-turn bouts (ARTR side, bout clock) | - |
| seeking | - | value cells: conjunctions of cue side x hunger band (CS-like, silent plastic synapses) onto a value cell per side. Value biases the turn (RS) toward the higher side and suppresses bouts while value rises (klinotaxis / klinokinesis) |

- **Why "seek when hungry" needs no wiring:** energy can only flow in below full. The conjunction cells (cue x low reserve) are paired with reward; (cue x full reserve) never are. The network learns from its own physiology that a cue matters when the reserve is low.
- **Caretaker phase:** early in life the world presents food at the mouth, or docks the robot, when the reserve runs low. The parent or owner can be an NPC teacher (`NM_MUJOCO_NPC_ACTOR`). The newborn feeds by reflex, and the cue is strongest exactly when energy rises, so the association forms before the body ever seeks.
  - The caretaker then withdraws: food stays where it grows, the dock stays on the wall.
  - A body that has learned the cue's value climbs its gradient during exploration. A naive body only stumbles on food.
- **Growth:** the conjunction cells are a neurogenic niche (clonal newborns, inputs fixed, outputs silent and plastic), the same rule that proved learning through growth on obstacle anticipation.
- **Proof** (same standard as growth):
  - experienced vs naive: time to food from random starts, and fraction of starts reaching food before the reserve is empty;
  - growth on vs off;
  - knockout of the learned value synapses and of the newborns, which returns the body to naive.

Per body:

| body | food / power | cue senses | new channels |
|---|---|---|---|
| C. elegans | bacterial lawn (`food_contact_*`, lawns) | `chem_left/right` (attractant), `taste_front_center` (exists) | `energy_reserve`, `consume` |
| Drosophila | fruit | antennal odour pair | `odour_l/r`, `energy_reserve`, `consume` (proboscis taste) |
| zebrafish | prey (14 items) | olfactory pair (the `olfactory_*` channels are 0.3 m range rays, not chemical) | true `odour_l/r`, `energy_reserve`, `consume` |
| crow | `food_0..7`, `water_bowl` | eyes, `beak_taste_*` (exist) | refill on beak contact; the existing need channels become real |
| hexapod, NAO, UGOT, Keenon, Botinkit | a charging dock | dock beacon (the existing `light_beacon`; hexapod camera on/off; a beacon receiver pair on bases that have only rays) | `battery`, `charge_current`, `beacon_l/r` where missing |

Work order:
- **F1 world:**
  - generalise the crow's energy to every profile (per-body capacity, basal and mechanical drain);
  - food and dock items with a `scene.stimuli` field per profile (generalise `celegans_stimuli`);
  - the feeding/charging transfer after `physics.step()` in `world.rs`, plus a caretaker mode;
  - new channels as an optional sensor group, so existing networks stay bound;
  - energy and feeding in the trace.
- **F2 network:** `aarnn_circuit.add_learned_foraging(...)`, holding the reward transient, feeding reflex, conjunction niche, value cells and steering bias, plus layout rules per anatomy.
- **F3 open loop:** a protocol like `body_learn_openloop.py` (caretaker sessions, then a gradient test).
- **F4:** closed loop per body.
- **F5:** the other engines.

**F1 done (25 Nov): energy, food and docks.**

Record (`scripts/embodiment_scene.py` `needs_block` / `needs_sensors`, regenerated with `scripts/embodiment.py`; `--check` clean):
- `scene.needs` (`needs-v1`) holds:
  - resource: food or power;
  - store = 1 W/kg x total mass x 240 s;
  - drain = basal x (1 + effort), effort = mechanical power / (2 W/kg x mass), at most 1;
  - refill from empty in 20 s at a source, only below full;
  - the sources;
  - a caretaker: until 600 s, below 40 % reserve, a 20 s feed.
- Sources per body:

  | body | sources | consumed within | cue field |
  |---|---|---|---|
  | C. elegans | 3 bacterial lawns + 4 food spots | 0.065 m + half the source | 0.23 m |
  | Drosophila (both) | 8 fruit / food items | 0.055 m + half | 0.19 m |
  | zebrafish | 14 prey | 0.0375 m + half | 0.13 m |
  | hexapod, UGOT, NAO, Keenon, Galbot | one charging dock: a 0.18-0.42 m floor pad (no collision) in the corner farthest from the furniture | 0.1 half extent + half the pad (0.22 m hexapod, 0.51 m room) | 0.35 half extent (0.46 m hexapod, 1.05 m room) |

  - None: the crow (keeps its own energy, hunger and taste channels) and the fixed-base machines (Cofe, Xbot, Botinkit).
- Optional sensor group `needs` (4 channels):
  - `needs_energy` (interoception / battery state of charge);
  - `needs_consume` (taste of food / charge current);
  - `needs_cue_left` / `needs_cue_right`: odour or dock-beacon receivers aimed 45 degrees outward, reading the strongest source's Gaussian field times `0.5 + 0.5 cos` of the bearing. A sum saturated among the worm's seven food sources.
  - Existing networks bind without the group (named binding), so no snapshot changes.

Engines:
- **MuJoCo** (`adapters/mujoco/src/world.rs` `NeedsState`) implements it fully. Each 2 ms step:
  - drain;
  - refill at a source or while the caretaker feeds;
  - caretaker onset;
  - exhaustion at zero, which disarms the body (a flat battery, an exhausted animal).
- **Trace:**
  - each row carries `needs`: energy, fraction, consume, caretaker feeding, source reached, cumulative feeds / visits / energy consumed, exhausted;
  - the header carries capacity, mass and sources.
- **Overrides:** `NM_MUJOCO_NEEDS=0` disables the model; `NM_MUJOCO_NEEDS_ENDURANCE_S`, `NM_MUJOCO_NEEDS_START` and `NM_MUJOCO_CARETAKER_S` override its parameters.
- **Other engines** have no energy model and report neutral needs channels (full reserve, no refill, no cue):
  - Webots (legacy and projection paths);
  - Isaac (`sim/isaacsim/senses.py`);
  - Unity / Unreal through the shared `aarnn_embodiment::projection` (FFI).

Checks:
- **Hexapod (fixed gait, no growth):** its 60 s walk is identical to its pre-needs run (1199 / 1199 trace rows).
  - With endurance shortened to 40 s, the store drains 1.0 -> 0.5 in 10 s of walking, and the caretaker charges it 3 times in a minute.
- **C. elegans:** spawns on a food spot (no inflow while full), drains to 0.89 in 25 s, and its cue pair now reads a gradient (left 0.64 / right 0.29 as it turns from the nearest food).
- **Tests:**
  - `aarnn-embodiment` 25;
  - `aarnn-mujoco` 14 + 4 (the C. elegans contract tests count 48 core channels plus the group);
  - simulator content 8;
  - Isaac parity;
  - Webots and FFI build.

### F2: learned foraging / recharging (25 Nov)

Circuit: `aarnn_circuit.add_learned_foraging`, built into the hexapod with `build_hexapod_cpg_network_json.py --forage 1`. Insect layout:
- hunger in the pars intercerebralis;
- feeding in the gustatory suboesophageal zone;
- reward from the PAM dopaminergic cluster;
- value at an appetitive mushroom-body output neuron;
- cue-band cells in the calyx;
- rise / fall cells in the LAL.

Innate parts (physiology, not seeking):
- **FEED** (`needs_consume`: food at the mouth, charge current; it flows only below a full reserve):
  - stops the walk command while feeding (consummatory reflex);
  - teaches the value cell V;
  - drives the reward cell DA_R, which joins LC in the plasticity gate and is classified dopaminergic for the neuromodulator pools.
- **HUNGER:** tonically on, silenced by SATED, a relay of the reserve at 0.5. It switches 14.7 Hz at reserve 0.30, 4 Hz at 0.45, 0 Hz from 0.50 up.
- **Run and turn:** RISE (V above its 3 s copy V_SLOW) suppresses the spontaneous turn-bout clock (keep running); FALL starts a turn bout (a pirouette).
  - Both need HUNGER: motivational state gates the use of a food memory, not its formation (Drosophila MB-MP1, Krashes et al. 2009).
  - Gating learning by hunger failed: hunger falls as soon as a meal refills the reserve, so cue and reward overlapped for only ~5 s per feed and the value learned 0.5 Hz instead of 19 Hz.

Learned part:
- 12 cue-band cells (6 intensity bands per side) project onto V through silent plastic synapses (oja, gated).
- The newborn niche covers the sonar and cue cells, with clonal newborns: inputs fixed, outputs silent and plastic. The builder's own mask first made newborn inputs plastic and their outputs fixed, and growth did nothing until that was corrected.

Open loop (`scripts/qa/forage_openloop.py`, Rust probe; `target/qa/forage/hex/ol_*`). Six caretaker cycles: the reserve drains 0.9 -> 0.35 over 40 s with faint wandering cues, then a 20 s feed (refill, both cues at 1). A frozen test follows: the cue rises 0 -> 1 over 20 s, holds, falls over 20 s, three times hungry (reserve 0.3) and three times sated (0.95).

| network | V at faint cue (< 0.25) / strong (>= 0.75) | cue at which V passes 2 Hz | hungry, cue rising: RISE / bout clock | hungry, cue falling: FALL / bout clock | sated |
|---|---|---|---|---|---|
| naive (before the caretaker) | 0.0 / 0.25 Hz | never | 0 / 5.9 Hz | 0 / 6.3 Hz | 0 / 6.5-6.9 Hz |
| learned, growth off | 0.48 / 18.9 Hz | 0.205 | 10.7 / **1.1 Hz** | 6.8 / **4.8 Hz** | RISE 0.04, clock 7.2 Hz (cue ignored) |
| learned + growth (3 seeds, +78 newborns) | **2.3-3.5** / 20.0 Hz | **0.12-0.14** | | | gated |
| growth, newborns silenced | 0.37 / 18.1 Hz | 0.23 | | | |

- Before the caretaker feeds, the cue means nothing: no value, no run / turn modulation, only exploration.
- After them, a hungry body runs on while the cue strengthens and turns when it fades (bout clock 1.1 vs 4.8 Hz): learned klinokinesis. A sated body keeps the memory but ignores it.
- Growth extends the learned value to fainter cues, i.e. detection from farther away (onset 0.12-0.14 vs 0.205). Silencing the newborns returns it to the no-growth level (0.23): learning through growth, on its third task.

Hunger gate, second version (both bodies rebuilt): HUNGER near saturation (tonic 14 mV/ms) and a 14 mV/ms gate. It now switches on below a reserve of about 0.35, not 0.5 (measured 26 Nov; see "The hunger threshold" under the foraging trials). With tonic 8 the Keenon's HUNGER reached only 9.8 Hz (its template keeps active dendrites), and a sated Keenon still ran on rising cues (bout clock 3.1 Hz up vs 7.5 Hz down). Re-measured, frozen tests after six caretaker feeds:

| body / network | hungry: bout clock while cue rises / falls | sated: rising / falling | cue at which V passes 2 Hz |
|---|---|---|---|
| hexapod, growth off | **1.0 / 4.7 Hz** | 7.7 / 7.4 Hz (ignored) | 0.205 |
| hexapod, growth (seed 7, +78) | 0.4 / 3.0 Hz | 7.2 / 6.7 Hz | **0.12** |
| hexapod, growth, newborns silenced | 1.6 / 5.8 Hz | 7.1 / 7.3 Hz | 0.23 |
| Keenon T8, growth off | **0.5 / 6.2 Hz** | 5.3 / 5.4 Hz (ignored) | 0.145 |
| Keenon T8, growth (seed 7, +80) | 0.0 / 1.0 Hz | 5.4 / 5.9 Hz | **0.063** |
| Keenon T8, growth, newborns silenced | 0.8 / 6.1 Hz | 5.3 / 5.4 Hz | 0.145 |

- Naive networks (before any feed): no RISE / FALL, the same bout clock on rising and falling cues (5.2-6.3 Hz), on both bodies.
- The Keenon runs the archetype explorer with spontaneous turn bouts (`--explore 1`) and the forage block (`--forage 1`); the learner fields now cover both learners (`aarnn_circuit.learner_fields`, `growth_niche_fields` with CS_ and CF_ parents).

Closed loop, first run (900 s, care until 400 s, endurance 160 s; `target/qa/forage/hex/walk_*_v1`):
- Both robots wedged against the slalom post 0.30 m from the dock from the start: the dock had been placed by centre distance to the furniture.
- The dock is now placed by edge clearance (hexapod: (-0.78, -0.26), 0.38 m from the nearest solid edge; records regenerated).
- Rerun (900 s, care until 400 s, default endurance; first hunger gate):
  - no growth and naive (bit-identical) walked into slalom post 3 (0.16 m from its centre) by 100 s and stayed there. 1.5 m from the dock they never sensed it (cue <= 0.05) and were exhausted at 488 s.
  - with growth (seed 7) the robot wandered: it reached the dock once at 87 s and after the caretaker came within 0.43 m (cue 0.66), but was exhausted at 494 s, 60 s after its hunger began.
  - The hexapod habitat tests its post blindness (the HC-SR04 cone misses posts) more than its foraging. Closed-loop foraging moves to the Keenon T8 in the open room (dock (-1.05, -0.9), cue field 1.05 m).

Keenon T8, closed loop, first round (900 s, care until 400 s; `target/qa/forage/keenon/walk_*_v1`; from 400 s):

| network | dock arrivals | charging | exhausted at | survived after care |
|---|---|---|---|---|
| learned, growth off | 1 | 1 s | 556 s | 156 s |
| naive (plasticity off) | 2 | 2 s | 591 s | 191 s |
| learned + growth (seed 7) | 12 | 19 s | 780 s | 380 s |

- The learned robots found the dock: growth off 9.9 s after its hunger began; growth 7.7 s and 36.6 s after its two hunger episodes.
- But on every arrival the base kept rolling at 0.13-0.28 m/s, grazed the pad's edge (0.38-0.46 m from its centre against a 0.51 m reach) and left within seconds.
- The feeding reflex had stopped only the forward command (MLR), while the turn cells (ARTR, RS) drive the Keenon's wheels directly.
- Feeding now halts locomotion as a whole (`add_learned_foraging(..., halt=[RS_l, RS_r, ARTR_l, ARTR_r])`, a worm dwelling on food).

Second round (feeding halts forward and turn drive; `walk_*_v2`; from 400 s):

| network | reserve at 400 s | arrivals | charging | hunger episodes that reached the dock | exhausted at |
|---|---|---|---|---|---|
| learned, growth off | 0.55 | 0 | 0 s | 0 of 1 | 499 s |
| naive | 0.99 | 6 | 8 s | 1 of 1 (34 s) | 665 s |
| growth, seed 5 | 0.59 | 5 | 7 s | 2 of 2 (8, 43 s) | 584 s |
| growth, seed 7 | 0.78 | 21 | 29 s | **4 of 4** (5, 22, 50, 74 s) | **834 s** |

- The growth robots seek the dock when hungry.
- Survival is confounded by the reserve each run had when care ended (0.55-0.99). Single runs; the comparison is not yet conclusive.
- Charging was still ~1.4 s per arrival. The trace shows why: the wheels stop within 0.3 s of arriving, then the stall reflex (STALL) reads the stopped base as blocked, and REV drives all four wheels backward at -0.7 about a second later, off the pad.
- Feeding now also silences STALL and REV.

Third round (feeding also silences STALL and REV; `walk_*_v3`):
- Every robot that reached the dock stayed on it.
- Cause: intake flowed whenever the reserve was below full. At full, intake stopped and the walk resumed. One wheel turn drained the reserve a fraction below full, intake restarted, and feeding halted the robot again, so it never left the pad.
- Fix (`needs-v1`, `refill_start_fraction`, default 0.9): intake starts only below 0.9 of full and then continues to full, or until the body leaves the source. A full body walks off and explores again.

Fourth round (refill hysteresis; `walk_*`, 25 Nov; `aarnn-mujoco --needs-report TRACE --from-s 400`):

| network | reserve at 400 s | arrivals | at the dock / charging | lowest reserve | hunger episodes (time to the dock) |
|---|---|---|---|---|---|
| learned, growth off | 0.86 | 10 | 76 s / 52 s | 0.56 | none |
| naive (plasticity off) | 1.00 | 7 | 67 s / 46 s | 0.24 | 1, reached (50.5 s) |
| growth, seed 7 | 0.86 | 12 | 79 s / 53 s | 0.58 | none |
| growth, seed 5 | 0.85 | 4 | 53 s / 42 s | 0.19 | 3: reached (59.4, 26.6 s), the third still open at 900 s |

- All four survive to 900 s. Charging now lasts the full refill (42-53 s in 4-12 visits, against ~1.4 s per visit before).
- **This round does not test seeking.** Every robot first reaches the dock at 13.5 s, on the same path. The dock lies on the room's habitual route, so the robots recharge in passing. Growth off and growth seed 7 never got hungry after care.
- A fair test needs releases away from the dock with the robot hungry: see "Foraging trials" below.

#### Foraging trials: hungry releases away from the food (26 Nov)

Trial mode in the MuJoCo adapter (`world.rs`, `struct Release`):
- `NM_MUJOCO_RELEASE_S` (period), `NM_MUJOCO_RELEASE_FROM_S` (first release), `NM_MUJOCO_RELEASE_ENERGY` (reserve at release), and `NM_MUJOCO_RELEASE_PLACES="x,y,yaw[,z];..."` (used in order).
- At each release the free root is put upright at the next place, velocities zeroed and the reserve reset. Joints and the brain carry over: the network keeps everything it learned.
- The trace records `needs.releases`. `aarnn-mujoco --needs-report TRACE --from-s 400` times each trial to the first arrival at a source and prints the share reached, the median time, and the mean with each miss counted as the full window.

Keenon protocol:
- Care until 400 s, then 12 releases.
- Six places 1.5-1.8 m from the dock (1.4-1.7 cue-field radii), each used twice.
- The robot always faces away from the dock (180 degrees) or sideways (+-90 degrees), never toward it.
- Control: the learned growth network (seed 7) released sated (reserve 1.0) at the same places.

**The hunger threshold.** The first round reset the reserve to 0.45, just under the circuit's nominal 0.5. Measured open loop (`target/qa/forage/*/hunger_curve`, reserve held 6 s per level), HUNGER is silent until the reserve falls below 0.40 on every body:

| reserve | 0.45 | 0.40 | 0.35 | 0.30 | 0.20 |
|---|---|---|---|---|---|
| Keenon T8 HUNGER / SATED (Hz) | 0 / 18.6 | 0 / 15.5 | 3.2 / 8.6 | 17.6 / 2.4 | 19.0 / 0 |
| Drosophila | 0 / 18.6 | 0 / 15.5 | 3.2 / 8.6 | 17.6 / 2.4 | 19.0 / 0 |
| zebrafish | 0 / 15.3 | 2.8 / 9.9 | 16.2 / 4.1 | 18.7 / 1.0 | 19.0 / 0 |

- The SATED relay still fires 15-19 Hz at reserves of 0.40-0.50. The second hunger gate (tonic 14, inhibition 24) moved the switch from 0.5 down to about 0.33.
- So a release at 0.45 left the robot sated for the first 20-30 s of each trial, until the drain took the reserve below 0.35.

Round 1, released at 0.45 and therefore mostly sated (`trials_mild`; stopped after 4-9 trials once the threshold was found):

| network | trials reached | median time to the dock | mean, misses counted as 50 s |
|---|---|---|---|
| learned, growth off | 5 of 9 | 6.6 s | 26.6 s |
| naive | 5 of 9 | 6.9 s | 26.4 s |
| growth, seed 7 | 3 of 5 | 18.2 s | 32.6 s |
| growth, seed 5 | 3 of 5 | 21.5 s | 30.1 s |
| growth seed 7, sated | 3 of 4 | 31.0 s | 30.2 s |

- As expected with hunger off, the learned and naive robots reach the dock equally often: the learned run / turn modulation is gated by hunger.
- Round 2 releases every body at 0.30 (HUNGER 17-19 Hz from the start), every 40 s (the fly every 36 s, because it drains fastest).

How the robots moved (`--needs-report` now also reports per trial, from the root pose and the source positions in the trace header):
- approach speed: the fall in distance to the dock per second before arrival;
- heading error to the dock (90 degrees = no preference);
- turn rate while getting nearer vs farther. This is measured by position, because the cue sensors depend on bearing and turning alone makes them rise.

| round 1 (0.45) | approach | heading error | turn rate nearer / farther |
|---|---|---|---|
| growth off | +0.108 m/s | 80 deg | 20.0 / 17.8 deg/s |
| naive | +0.107 m/s | 79 deg | 18.9 / 16.0 deg/s |
| growth, seed 7 | +0.081 m/s | 82 deg | **18.5 / 21.2** deg/s |
| growth, seed 5 | +0.082 m/s | 78 deg | **15.2 / 19.4** deg/s |
| growth seed 7, sated | +0.092 m/s | 71 deg | 19.0 / 16.8 deg/s |

- Only the two growth learners turn more while moving away from the dock than toward it: klinokinesis in closed loop. The effect is small. Naive, growth off and sated turn more while approaching.
- The misses show the limit of bout timing alone. In one, the robot ran past at 0.3 m/s, 0.59 m from the dock centre (reach 0.51 m), with its left cue at 0.83, and drove on. Nothing in the circuit steers toward the stronger side.

Round 2, hungry releases (reserve 0.30, 12 trials every 40 s from 400 s; `keenon/trials`, `report.txt`):

| network | trials reached | median | mean, misses = 40 s | approach | heading error | turn rate nearer / farther |
|---|---|---|---|---|---|---|
| learned, growth off | 3 of 12 | 29.5 s | 35.6 s | +0.034 m/s | 89 deg | 15.7 / 15.3 deg/s |
| naive | **9 of 12** | 6.5 s | **18.7 s** | +0.136 m/s | 71 deg | 17.5 / 13.6 deg/s |
| growth, seed 7 | 6 of 12 | 20.1 s | 29.7 s | +0.057 m/s | 88 deg | **15.9 / 19.8** deg/s |
| growth, seed 5 | 6 of 12 | 31.8 s | 35.0 s | +0.023 m/s | 89 deg | **15.0 / 21.9** deg/s |
| growth seed 7, released sated | 8 of 12 | 16.7 s | 24.2 s | +0.092 m/s | 80 deg | 20.1 / 17.8 deg/s |

- **Learned klinokinesis alone does not get the robot to the dock.** The naive network reaches it most often, and the same growth network does better sated than hungry.
- The learned effect is real and replicates across both rounds and both seeds: hungry growth learners turn 25-46 % more while moving away from the dock than toward it. No other network does.
- But in this room the outcome of a trial is decided in its first seconds, by which way the robot's spontaneous turn goes after release.
  - Example, the 440 s release with the dock 90 degrees to the right (right cue 0.34, left 0.04): the naive robot happened to turn right and docked in 4 s. The growth-off learner turned left; its cue fell to 0 and it drove on for 8 s before its next bout.
  - Extra turning when the value falls cannot undo a wrong first turn quickly. In open floor it costs travel, so hungry learners cover less ground than sated ones.
- The misses are not exhaustion: no robot ran out, because each release resets the reserve.

**Learned steering (klinotaxis)**, `add_learned_foraging(..., steer=True)`, Keenon `--forage-steer 5` (`target/qa/forage/keenon_steer`):
- V_l / V_r: one value cell per side, taught by FEED like V, each learning through silent plastic synapses from that side's cue bands.
- STEER_l / STEER_r: V_l - V_r and V_r - V_l, gated by HUNGER, silenced while feeding. Each drives a graded wheel push-pull toward its side.
- The value is learned; only the sensorimotor wiring is innate, as a worm's head-swing comparison or a fly's bilateral antennal comparison.

Open loop (`keenon_steer/steer_openloop.py`; six caretaker feeds, then frozen 8 s blocks):

| reserve, cue left / right | naive: STEER_l / STEER_r | learned: V_l / V_r | learned: STEER_l / STEER_r |
|---|---|---|---|
| 0.30, 0.60 / 0.10 | 4.4 / 3.8 Hz | 13.5 / 0.3 Hz | **19.8 / 0.0 Hz** |
| 0.30, 0.10 / 0.60 | 4.2 / 4.8 Hz | 0.3 / 13.5 Hz | **0.0 / 19.8 Hz** |
| 0.30, 0.35 / 0.35 | 4.5 / 4.5 Hz | 5.7 / 5.7 Hz | 13.7 / 13.7 Hz (cancel) |
| 0.95, 0.60 / 0.10 | 0 / 0 | 13.5 / 0.3 Hz | 2.7 / 0.0 Hz |

- Naive, the cue does nothing: the value cells are silent and the steering cells sit at their symmetric hungry baseline, which cancels at the wheels.
- Learned and hungry, the robot turns toward the side whose cue predicts the dock. Sated, the turn is almost gone (2.7 Hz).

Closed loop, the same hungry releases as round 2 (reserve 0.30, 12 trials every 40 s; `keenon_steer/trials`):

| network (with learned steering) | trials reached | median | mean, misses = 40 s | approach | heading error |
|---|---|---|---|---|---|
| learned, growth off | **12 of 12** | **4.6 s** | **9.5 s** | +0.228 m/s | 64 deg |
| naive (same network, plasticity off) | 7 of 12 | 23.1 s | 27.2 s | +0.086 m/s | 83 deg |
| growth, seed 7 | **12 of 12** | 11.0 s | 16.1 s | +0.128 m/s | 83 deg |

- **This is the closed-loop proof of learned seeking.** The two networks differ only in whether they learn. The learned robot found the dock in every trial, 3x faster on average.
- With growth (seed 7) the learner also docked in every trial, but more slowly than without growth (mean 16.1 s against 9.5 s). Growth widens the learned cue range in open loop (detection from 0.063 instead of 0.145); here that does not translate into faster docking. Its earlier, wider value may start the steering turn from farther away, where the bilateral difference is small. One seed only.
- The naive robot's 7 arrivals are the exploration baseline (round 2: naive 9 of 12, growth off 3 of 12 without steering).
- After its first feeds the learned robot docked 13 times after care and was 0.94 full at the end; the naive one docked 7 times.
- Steering replaces nothing innate. Feeding, hunger and exploration are unchanged; what the robot learned from the caretaker's feeds is which way to turn.

Fish and fly (`--explore 1 --forage 1` on `build_zebrafish_swim_network_json.py` / `build_drosophila_walk_network_json.py`; cells placed by `layout_designed_circuit.forage_rule`: hypothalamus, vagal lobe / SEZ, posterior tuberculum / PAM, ventral striatum / appetitive MBON, olfactory bulb / antennal lobe), open loop, growth off:

| body | hungry: bout clock rising / falling | sated: rising / falling | cue at which V passes 2 Hz |
|---|---|---|---|
| zebrafish | **0.9 / 7.4 Hz** | 5.9 / 6.0 Hz | 0.19 |
| Drosophila (BANC walk) | **0.5 / 6.2 Hz** | 5.3 / 5.4 Hz | 0.145 |

- Zebrafish with growth (seed 7, +80 newborns): V passes 2 Hz at cue **0.112** (no growth 0.19); hungry bout clock 0.3 Hz rising / 3.1 Hz falling; sated 6.2 / 6.0 Hz. With the newborns silenced: 0.20, back to the no-growth level.
- Drosophila with growth (seed 7, +80): V passes 2 Hz at cue **0.097** (no growth 0.145); hungry bout clock 0.15 / 2.4 Hz; sated 5.2 / 5.8 Hz. With the newborns silenced: 0.145.
- Growth extends the learned food / dock value to fainter cues on all four bodies tested, and silencing the newborns removes the gain each time:

  | body | no growth | growth | newborns silenced |
  |---|---|---|---|
  | hexapod | 0.205 | 0.12-0.14 (3 seeds) | 0.23 |
  | Keenon T8 | 0.145 | 0.063 | 0.145 |
  | zebrafish | 0.19 | 0.112 | 0.20 |
  | Drosophila | 0.145 | 0.097 | 0.145 |

- The block and its bout clock are identical on every body, so in open loop the fly matches the Keenon closely.
- The worm has no designed hindbrain to attach the block to, and no growth. It learns in its own connectome instead (next section).

Fish and fly with learned steering (`--forage-steer`: fish 2, fly 3; `target/qa/forage/{fish,fly}_steer`):
- Wiring, innate:
  - fish: STEER_l drives the right tail motors and STEER_r the left ones, so the tail bends toward the steering side;
  - fly: each STEER cell inhibits its own side's coxa channels, so the fly turns toward that side.
- Open loop (six caretaker feeds, then frozen 8 s blocks; `ol.log`). Hungry (reserve 0.30), cue 0.60 on one side and 0.10 on the other:

  | body | naive: STEER toward / away from the cue | learned: V toward / away | learned: STEER toward / away | learned, sated (0.95): STEER toward |
  |---|---|---|---|---|
  | zebrafish | 2.6 / 2.3 Hz | 8.1-8.6 / 0.1 Hz | **18.8 / 0.0 Hz** | 0.0 Hz |
  | Drosophila | 4.2-4.8 / 3.8-4.2 Hz | 13.6 / 0.3 Hz | **19.8 / 0.0 Hz** | 2.7 Hz |

Closed loop, hungry releases. Care until 400 s; then 12 releases (fish every 40 s, fly every 36 s), each 0.18-0.24 m from the nearest prey, never facing it, at reserve 0.30, with the brain persisting:

| zebrafish network (with learned steering) | trials reached | median | mean, misses = 40 s | approach | heading error |
|---|---|---|---|---|---|
| learned, growth off | **10 of 12** | **4.3 s** | **10.9 s** | +0.037 m/s | 52 deg |
| naive (same network, plasticity off) | 5 of 12 | 6.2 s | 28.8 s | +0.010 m/s | 110 deg |
| growth, seed 7 | **12 of 12** | 5.9 s | **6.7 s** | +0.036 m/s | 55 deg |

- The fish replicates the Keenon result. The learned fish reached prey twice as often as the naive one, and 2.6x faster on average. With growth it reached prey every time, and its mean time was the lowest of the three.
- After care, the growth fish arrived 130 times, ended 0.95 full, and its reserve never fell below 0.21. The learner arrived 74 times (end 0.80) and the naive fish 31 times (end 0.69).
- A miss is a fish stuck at the tank wall, not one swimming in the wrong direction:
  - in both of the learner's misses, and in 5 of the naive fish's 7, the fish sat against the wall (x or y at +-0.36 m), or in a corner, for the rest of the window;
  - it bobbed between the floor and mid-water (z 0.01-0.11 m), with approach at 0;
  - arrival counts the horizontal distance, so depth cannot cause a miss.
- The naive fish's first trial overlapped a caretaker feed by 0.55 s (fixed since: a release now ends any feed).

Drosophila, first set (`fly_steer/trials`): learned 3 of 12 (mean 30.8 s), naive 1 of 12 (33.4 s), growth (seed 7) 3 of 12 (28.7 s). This set does not test learning:
- 5 of its 12 release points lay 0.05-0.08 m from the arena wall (+-0.52 m), and the heading chosen to avoid facing the fruit pointed the fly at that wall.
- In most trials the fly walked into the wall within 2-6 s and stayed against it, tilted 55 deg (up-vector 0.55-0.60), creeping along it at about 0.005 m/s until the window ended. Both networks did this.
- Path per trial: 0.1-0.5 m, against about 2 m in a free 36 s window.
- The learner's first trial also overlapped the caretaker's last feed (fixed since).

Drosophila, releases clear of the walls (`fly_steer/trials_open`, `run_fly_open_trials.sh`): 12 places 0.26-0.32 m from the nearest fruit, heading 110-130 deg off its bearing, at least 0.37 m of floor ahead (`fly_places_open.txt`); otherwise identical:

| Drosophila network (with learned steering) | trials reached | median | mean, misses = 36 s | approach | heading error | windows touching a wall / time in contact |
|---|---|---|---|---|---|---|
| learned, growth off | 4 of 12 | 5.4 s | 25.8 s | +0.015 m/s | 98 deg | 9 of 12 / 43 % |
| naive (plasticity off) | **8 of 12** | 7.4 s | **16.5 s** | +0.025 m/s | 82 deg | 5 of 12 / 6 % |
| growth, seed 7 | 4 of 12 | 5.9 s | 25.9 s | +0.015 m/s | 96 deg | 9 of 12 / 14 % |

- **The fly shows no learned seeking; the naive fly did best.** The learners walked straight to a wall within 5-14 s of release in 9 of 12 trials, heading 110-150 deg from the fruit, and stayed there for 20-30 s. The naive fly wandered onto fruit in about 7 s.
- **The fly's learned steering does not turn it.** Turn rate while hungry and moving, by the left-right cue difference (from the trial traces):
  - fly learner: -3 to -28 deg/s (a constant rightward drift) at every cue difference, as in the naive fly;
  - zebrafish learner: +43 deg/s with the left cue 0.3 stronger and -68 deg/s with the right cue 0.3 stronger, so it turns toward the food (the naive fish shows no relation).
  - The fly's STEER output cuts its own side's coxa strides (-3 mV/ms per channel). The touch escape showed that cutting strides does not turn this fly body against a wall.
- Forced-steering test (`target/qa/forage/wall/steer_test`): STEER_l held on, sated fly released at the arena centre in 8 headings, net turn rate per 10 s (+ = left):

  | fly | net turn rate | path per release |
  |---|---|---|
  | control (steering off) | mean -24 deg/s (-37 to -3) | 0.53-0.82 m |
  | STEER_l on, the trials' strength (cut 3 mV/ms) | mean -12 deg/s (-27 to +4) | 0.39-0.71 m |
  | STEER_l on, 4x (cut 12 mV/ms) | **mean +11 deg/s (+1 to +16, all 8 releases)** | 0.33-0.38 m |

  - At the trials' strength the learned steering shifts the heading by only 12 deg/s, half the fly's drift. At 4x it shifts it by 35 deg/s, enough to turn left against the drift in every release, but the fly walks at half speed. Hence `--forage-steer 12` for the fly.
  - Every fly drifts right at about 25 deg/s, a full circle every 14 s. This predates the foraging and touch blocks. ARTR alternates fairly (each side leads about half of the 5 s blocks), so the drift is probably in the gait; open.

**Touch escape: fish and fly get off walls (26 Nov).** Both animals got stuck at walls: the fish in a tank corner, sinking, and the fly rearing against the arena wall.

Why. Both already had an innate turn away from what their range rays see (rays -> the hindbrain RS cells). Open loop (`target/qa/forage/wall/wall_openloop.py`, rays held at wall values), with a wall ahead:
- RS_l and RS_r both ran at 19.4 Hz in the fish and 19.9 Hz in the fly, so the two turns cancelled.
- Each side's drive (the rays plus the learned anticipation cells LT, about 10 mV/ms) exceeded the 6 mV/ms mutual inhibition that should let only one side win.
- The fish's bends cancelled and it held its head on the wall. The fly shortened every stride, stalled, and its front legs climbed the wall.

The reflex, `aarnn_circuit.add_touch_escape` (builders: `--touch-escape 1`):
- Touch:
  - each touch channel has a relay (`TCH_<side>_<k>`);
  - the relays of a side converge on a saturating touch cell (`TOUCH_l` / `_r` / `_f`, head-on to both sides);
  - fish: a surface within 3 cm of the snout (head rays >= 0.9, the near field of the head's lateral line and trigeminal touch);
  - fly: within 4.4 cm of the thorax (front rays within 60 deg of ahead >= 0.45, about 1 cm beyond the head: the antennae and front legs) and the thorax's own touch sensors.
- Escape cells `ESC_l` / `ESC_r`:
  - bistable with 30 mV/ms mutual inhibition, so one side fires, like the paired Mauthner cells in fish and the paired escape descending neurons in insects;
  - head-on, ARTR's standing bias picks the side.
- Each escape:
  - excites its own side's RS (turn away);
  - silences the opposite RS and the learned steering;
  - fish: bends the tail on its own side, a C-bend;
  - fly: pivots. The inner legs of the turn step backwards through AND cells (`REV_<leg>_<phase>`, on only while the leg's swing or stance cell and the escape both fire), as the moonwalker descending neurons do (Bidaye et al. 2014), while the outer legs walk on.
- Feeding (FEED) silences the escape, so the fly can stand at the solid fruit. The fish's prey is invisible to its rays.
- The touch cells decay slowly (fish 400 ms, fly 1000 ms; `--touch-tau`), so the turn carries on after contact ends.
- Layout: Mauthner cells in rhombomere 4, escape descending neurons in the GNG, touch relays in the trigeminal ganglion and the AMMC.
- Off by default: the complexity ladder stimulates the fish's rays at 0.9. Without the flag both builders reproduce their earlier networks exactly.

Open loop, after the fix (both bodies): one escape cell fires for every wall and corner, on the side of the nearer wall. No escape fires in open water, and the turn cells are on as often as without the block (fish RS_l 17 %, fly 7 %; `wall/open_water.py`).

Closed loop (`target/qa/forage/wall/closed/run.sh`): 8 releases every 15 s, sated, facing a wall head-on, at 30 deg or into a corner. Report: `aarnn-mujoco --wall-report TRACE --half-m H --contact-m C` (Rust; contact is the root within 2 cm of the wall for the fish, 4 cm for the fly):

| body | releases touching a wall | time in contact | farthest from the wall after release | most tilted (up-z) |
|---|---|---|---|---|
| zebrafish, no escape | 6 of 8 (1 cleared) | 38 % | stuck at the wall in 7 of 8 | 0.80 |
| **zebrafish, touch escape** | **0 of 8** | **0 %** | turns away at 3 cm; ends 0.16-0.28 m away | 0.69 |
| Drosophila, no escape | 8 of 8 (0 cleared) | 36 % | 0.04-0.06 m: pinned, path 0.2 m per release | 0.50 (rears) |
| Drosophila, escape, touch 400 ms | 2 of 8 | 0 % | 0.08-0.09 m: follows the wall, path 0.5-0.6 m | 0.86 |
| **Drosophila, escape, touch 1000 ms** | 3 of 8 (0.2-0.3 s each) | **1 %** | **0.25-0.48 m in 7 of 8**, path 0.8-1.2 m | 0.50 (once) |

Tried and rejected:
- **Wiring each relay straight to ESC.** The fly has 6 relays per side, which beat the mutual inhibition, so both escapes fired. Hence the saturating per-side touch cells.
- **Turning by cutting the inner strides.** With its head on the wall, the outer legs' forward push pressed that side into the wall. Hence the backward-stepping pivot. The inner front and hind legs reverse; the inner middle leg kept stepping forward, although it is wired like the others.
- **A short escape (touch decay 10 ms).** The fly let go as soon as its antennae cleared the wall, crept back and touched again, holding a fixed 37 deg angle: a 1.7 cm back-and-forth with no rotation.
- **A more decisive turn decision (RS mutual inhibition 6 -> 20).** In open water it held RS_l on 75 % of the time in the fish (17 % before) and 32 % in the fly (7 %), so the body circled.
- **A self-sustaining escape (a tonic inside ESC's bistable band).** At rest the escape cells leaked 1-2 Hz and their lead made it asymmetric, with the same circling bias.

Foraging trials with the touch escape (`run_touch_trials.sh`, `target/qa/forage/{fish,fly}_touch/trials`): the steering trials' protocol and places, including the fly's original wall-adjacent releases. The fly's learned steering is 4x stronger (`--forage-steer 12`; see the forced-steering test above).

Zebrafish (`fish_touch/trials`, touch decay 400 ms; the steering trials without the escape in brackets):

| zebrafish network (learned steering + touch escape) | trials reached | median | mean, misses = 40 s | heading error | after care: arrivals, lowest / end reserve |
|---|---|---|---|---|---|
| learned, growth off | **12 of 12** (10) | **5.0 s** (4.3) | **5.5 s** (10.9) | **39 deg** (52) | 101, 0.25 / 0.86 |
| naive (plasticity off) | 10 of 12 (5) | 16.8 s (6.2) | 19.8 s (28.8) | 73 deg (110) | 46, 0.10 / 0.73 |
| growth, seed 7 | 9 of 12 (12) | 7.8 s (5.9) | 16.7 s (6.7) | 62 deg (55) | 79, 0.10 / 0.63 |

- **No fish touched a wall in any of the 36 trials.** Without the escape the head rays read a wall (>= 0.9) for up to 86-99 % of a window; with it, for 0-2 % (learner and naive fish).
- **The learned fish reached prey in every trial, 3.6x faster on average than the naive fish,** with its heading 34 deg closer to the prey. The naive fish's misses had mostly been wall trapping, so it now reached 10 of 12, but it still took 3x longer (median 16.8 s against 5.0 s).
- **The growth fish got worse: 3 misses** (released at 400, 480 and 600 s).
  - This fish lives on the tank floor: its root was below 5 cm for 62-88 % of every window, and for 58-89 % in the run without the escape. Floor-dwelling is a trait of the grown network, not of the escape.
  - In its three misses it circled slowly (0.036-0.041 m/s, 30-60 deg/s) on the floor about 4 cm from a wall. There its head rays read the wall in 9-19 % of samples, against 0-9 % in the trials it reached, so repeated escapes may have turned it in place.
  - Without the escape the same floor-bound fish had reached every trial. One seed, 12 trials per network; open.

Drosophila (`fly_touch/trials`, touch decay 1000 ms, steering 12 mV/ms; the first set's places, 5 of 12 near a wall; first set in brackets):

| Drosophila network (learned steering 4x + touch escape) | trials reached | median | mean, misses = 36 s | heading error | windows touching a wall / time in contact | walking speed before arrival |
|---|---|---|---|---|---|---|
| learned, growth off | 9 of 12 (3) | 14.7 s | 21.0 s (30.8) | **60 deg** | 3 of 12 / 0 % | 0.013-0.051 m/s |
| naive (plasticity off) | **10 of 12** (1) | **8.3 s** | **15.9 s** (33.4) | 74 deg | 6 of 12 / 0 % | 0.024-0.077 m/s |
| growth, seed 7 | 4 of 12 (3) | 11.6 s | 27.8 s (28.7) | 71 deg | 0 of 12 / 0 % | 0.009-0.039 m/s |

- **The escape freed the fly from the walls.** The same places that pinned it in the first set (3, 1 and 3 of 12 reached) gave 9, 10 and 4 of 12. Wall touches were brief: time in contact rounds to 0 % in every run (36 % without the escape).
- **The stronger learned steering now turns the fly toward food.** Turn rate while hungry and moving, by the left-right cue difference (-0.3 = right cue stronger):

  | fly | cue -0.3 | cue 0 | cue +0.3 |
  |---|---|---|---|
  | learner, steering 12 | -13 deg/s | 0 deg/s | +5 deg/s |
  | growth, seed 7, steering 12 | -9 deg/s | +7 deg/s | +2 deg/s |
  | naive | -15 deg/s | -15 deg/s | -8 deg/s |
  | learner, steering 3 (`trials_open`) | -9 deg/s | -13 deg/s | -9 deg/s |

  The learner also loses its rightward drift (0 deg/s at equal cues).
- **It still does not beat the naive fly.** Its heading is 14 deg closer to the fruit, but it walks at about half the naive fly's speed and takes longer on average (21.0 s against 15.9 s). Steering by cutting strides slows the fly (the forced-steering test), and the learner's lost drift at equal cues suggests both sides' steering is partly on while hungry. A turn without the speed loss needs a steering output that turns the fly without shortening its strides, such as the escape's backward-stepping pivot; open.
- The growth fly walked slowest (0.009-0.039 m/s) and missed 8 of 12 trials away from any wall, its approach close to 0.

**Fly food steering by the pivot (26 Nov evening).** The fly's learned steering now turns it the way the touch escape does (`--forage-steer-mode pivot`; `cut`, the default, keeps the stride cut):
- STEER_l (odour stronger on the left) drives the left legs' reversal AND cells (`REV_<leg>_<phase>`, shared with the escape). Those legs step backwards while STEER_l and their phase cell both fire, and the right legs walk on, so the fly turns left.
- Each reversing swing also silences that leg's forward protraction (`REV_<leg>_sw -> D_<leg>_sw`, `--pivot-protract` 30 mV/ms), which would otherwise cancel the backward swing.
- `--forage-steer` is now the STEER -> REV weight (mV/ms). Cut-mode builds are unchanged: the rebuilt `fly_touch` and `fly_steer` networks match the old ones exactly.

Forced-steering test, as above (`target/qa/forage/wall/pivot_test`; STEER_l held on, sated fly at the centre, 8 headings):

| fly | net turn rate (+ = left) | path per 10 s |
|---|---|---|
| control (steering off) | -24 deg/s | 0.53-0.82 m |
| stride cut 12 mV/ms | +11 deg/s | 0.33-0.38 m |
| pivot, weight 10, STEER fully on | **+43 deg/s (all 8)** | 0.22-0.23 m |
| pivot, weight 10, STEER partly on (tonic 6) | +45 deg/s | 0.29-0.30 m |
| pivot, weight 20, partly on | +39 deg/s | 0.19-0.21 m |
| pivot, weight 5, fully on | +42 deg/s | 0.29-0.30 m |

- The pivot shifts the heading by about 67 deg/s, twice the 4x stride cut, and it saturates: weight and STEER level barely change it.
- While it is held on, the fly turns nearly in place. In foraging, a turn this fast should leave STEER on only briefly, until the cues balance, after which the fly walks at full speed.

Foraging trials (`run_touch_trials.sh fly pivot` and `fly pivot5`; `fly_pivot` weight 10, `fly_pivot5` weight 5; touch escape on; same places and protocol as `fly_touch`):

| Drosophila network | trials reached | mean, misses = 36 s | turn rate at cue -0.3 / 0 / +0.3 | walking speed before arrival |
|---|---|---|---|---|
| learned, pivot weight 10 | **0 of 12** | 36.0 s | -12 / +4 / +46 deg/s | 0.006-0.043 m/s |
| naive, pivot weight 10 | 2 of 12 | 35.2 s | +1 / +2 / +1 deg/s | 0.008-0.022 m/s |
| learned, pivot weight 5 | **0 of 12** | 36.0 s | -22 / -1 / +42 deg/s | 0.004-0.014 m/s |
| naive, pivot weight 5 | 10 of 12 | 13.4 s | -12 / -13 / -13 deg/s | 0.045-0.075 m/s |
| learned, stride cut 12 (above) | 9 of 12 | 21.0 s | -13 / 0 / +5 deg/s | about 0.035 m/s |

- **The pivot turns the learned fly toward food,** about 4x the stride cut's cue dependence, and with no drift at equal cues.
- **But it stops the fly reaching food.** While STEER is on, the fly turns nearly in place (0.004-0.04 m/s), so no learned fly reached fruit at either weight. The forced test above showed the same thing: a pivot held on gives a path of 0.2-0.3 m per 10 s.
- **The pivot also stalls the naive fly at weight 10** (2 of 12, 0.01-0.02 m/s). The AND cells open at STEER release of about 0.2, so even the untrained steering's small resting release pivots the legs. At weight 5 (threshold about 0.4) the naive fly walks normally and reaches 10 of 12.
- **Conclusion:** the stride cut stays the better food steering for foraging. A useful pivot needs a short, bounded turn followed by forward walking, for example the pivot gated by a turn-bout cell, or driven only by large value differences. This is open.
- Growth (seed 7) networks also fail with the pivot: 0 of 12 at weight 10 and 1 of 12 at weight 5, at 0.005-0.047 m/s.

For foraging builds use `--touch-escape 1` (fish `--touch-tau 400`, fly `--touch-tau 1000`). It stays off by default because the complexity ladder stimulates the fish's rays at 0.9.

**C. elegans: learning to seek food in the connectome (26 Nov).** The worm keeps its imported wiring. It already senses food through its own channels: soluble taste reaches ASE / ASG / ASI / ADF, and the lawns' odour reaches AWA / AWC. The changes:
- The caretaker's feed now reaches those senses. While it feeds, the worm's attractant taste and odour fields read 1 (food at the nose, `celegans_sensors`). Before, the feed only refilled the reserve, and the worm never experienced being fed.
- Plasticity only at the identified sensory-interneuron synapses. Only the synapses from AWA, AWC and ASE onto AIY, AIA, AIB and AIZ learn (26 synapses, `aarnn_plastic_cells` with `pre_hidden`). This is where C. elegans food-odour and salt learning sits; locomotion circuits stay fixed.
- Gated by the dopamine pool. The worm's dopaminergic cells (CEP, ADE, PDE) sense bacteria mechanically, which the habitat does not model, and their rates do not follow food here. The neuromodulator pools do: food raises dopamine through the sensory drives (1.42 between feeds, 1.69 while fed).
  - New runtime knobs `aarnn_modulator_pool` / `_threshold` / `_ref`: without marked modulatory cells, plasticity runs at (pool level - threshold) / ref, clamped 0-3.
  - With threshold 1.5 the gate is open 99 % of the time while fed and 6 % between feeds.
  - Unit test `pool_gated_plasticity_follows_the_pool_level`.
- The rule is STDP. Oja collapsed the spiking connectome's weights: its equilibrium is P(pre spike | post spike), about 0.01 at 5-10 Hz with 1 ms steps, so 6 feeds cut all 26 synapses by 60-70 % (`worm/ol_oja`). Hebb only counts spikes in the same 1 ms step.

Open loop (`scripts/qa/celegans_forage_openloop.py`, `target/qa/forage/worm/ol_stdp`; 3 caretaker feeds, then frozen odour ramps from the habitat background 0.33 to 1, three times):

| | before | after 3 feeds |
|---|---|---|
| AWA / AWC / ASE -> AIY (sum of weights) | 1.649 | 2.019 (+22 %) |
| -> AIZ / AIB / AIA | 0.486 / 0.730 / 0.432 | 0.608 / 0.833 / 0.471 |
| AIY at background / high odour | 7.4 / 8.2 Hz | 7.5 / 8.9 Hz |
| AVB (forward command) at background / high odour | 8.6 / **6.9** Hz | 7.0 / **8.4** Hz |
| RIA at background / high odour | 9.6 / 10.6 Hz | 10.1 / 12.0 Hz |

- 20 of the 26 synapses strengthened. The largest gains were AWCL -> AIBL (x4.3), AWCR -> AIYL (+53 %), AWAR -> AIZR (+32 %) and AWCL -> AIYL (+31 %).
- After learning, strong odour raises the forward command AVB instead of lowering it, so the worm runs on in food odour.
- The effects are small against the cells' 5-12 Hz spontaneous activity, and this is one run.
- Closed-loop trials, learner vs naive connectome (`worm/trials`): care until 400 s, then 12 releases every 60 s, 0.11-0.16 m from the nearest lawn edge, at reserve 0.30:

  | connectome | trials reached | median | mean, misses = 60 s | approach |
  |---|---|---|---|---|
  | learner (pool-gated STDP) | 8 of 12 | 20.8 s | 35.1 s | +0.020 m/s |
  | naive (plasticity off) | 8 of 12 | 25.0 s | 37.5 s | +0.003 m/s |

  - **No learned seeking is measurable in closed loop.** Each run has one trial that does not count: it started on a lawn and was "reached" at 0 s (learner 1060 s; naive 400 s, where the caretaker's last feed was still running: these runs predate the fix that ends a feed at a release). Without them, both connectomes reached 7 of 11.
  - The learner's mean approach is pulled up by one trial (+0.19 m/s at 460 s, reached in 1 s from 0.24 m), which the worm cannot crawl. Without it the two connectomes approach alike, at 0.002-0.008 m/s.
  - At 0.005 m/s a lawn 0.1 m away takes about 20 s even on a straight path. The open-loop change (AVB +1.5 Hz with strong odour) is too small against the 5-12 Hz spontaneous activity to bend the path in 60 s.
  - Next: more feeds before the trials, a stronger learning rate or more plastic synapses (e.g. onto AIY's targets RIA / AIZ -> SMB), and more trials.

Hexapod without the slalom posts (adapter knob `NM_MUJOCO_REMOVE_GEOMS=habitat_slalom_`, `Physics::remove_geoms`: 4 posts moved out of the world):
- Growth off and naive roam the east half and from ~300 s stay against the east wall (x ~ 1.08). They never come within 0.75 m of the dock and are exhausted at 488 s.
- The drain is identical with and without posts: walking keeps the effort term saturated, so the energy timeline does not depend on the path.
- Hexapod closed-loop foraging is limited by its wall and post wedging, not by what it learned.
- Report: `aarnn-mujoco --needs-report TRACE --from-s 400` (Rust):
  - feeds, arrivals and energy taken in;
  - each hunger episode timed to the source, the caretaker or exhaustion.

### Neuromodulators in synaptic transmission: audit and plan (25 Nov)

The user's requirements (25 Nov):
- Dopamine and serotonin, among others, set how well stimuli cross the synapse.
- They must act in the bouton -> synapse -> dendrite calculation in every AARNN network.
- The UI and the simulator both need a graph of global dopamine, serotonin and stimuli against synapse energy.
- Food, light, heat and activity each affect these levels.
- Their use depends on the depth tier auto-selected at the time.

**What exists (audit, 25 Nov).** The transmission path does not use them.

Modulators:
- **What there is:** three global scalars, `Runner.neuromod_dopamine`, `neuromod_ach` and `neuromod_serotonin` (`src/runner.rs:1585-1587`).
- **Update:** an EMA toward baseline plus gain x signal, clamped 0..3 (`runner.rs:11120-11236`).
- **Defaults:** baselines 1.0 and every signal gain 0.0, so the levels never move. Only `network_nao.json` sets gains.
- **Missing transmitters:** no noradrenaline or octopamine. C. elegans has octopamine/tyramine cell types whose transmitter identity is ignored.

Where they act today:
- **Plasticity:** DA/5-HT scales the learning rate (`runner.rs:10716-10724`, used at 16493).
- **ACh, GPU only:** dynamic ACh scales postsynaptic gain in the OpenCL kernels only. The CPU filter uses the static `bio.acetylcholine_gain` (`runner.rs:2464-2485`, `src/aarnn/dynamics.rs:165-169`), so the CPU and GPU paths diverge.
- **Volume transmission:** active cells whose type name contains "neuromod" or "vip" scale the total hidden input current. It is off by default and on in the C. elegans and Drosophila networks (`dynamics.rs:417-503`, `runner.rs:3434-3497`, 12593).
- **Learner networks:** the LC teaching cell is a separate mechanism (`modulated_plasticity_gate`, `runner.rs:10225-10263`) that only scales the learning rate. `apply_learner` switches `neuromodulation_enabled` off on every type (`scripts/aarnn_circuit.py:1025`).

The transmission path itself:
- Release, `should_release` (`runner.rs:2639` -> `src/aarnn/plasticity.rs:337-374`), uses a global `p_release_default`. The per-synapse `Synapse.p_release` is unused.
- Delay and attenuation (`src/aarnn/transmission.rs:143-201`) and short-term plasticity (STP) have no modulator term.
- The morphology route sums `w * atten * stp_scale` (`runner.rs:12333-12362`, 12182-12198, 14248-14690, 2653).

Depth tiers and drivers:
- Depth >= 1 enables synaptic filters, STP and the neuromod gains. Depth >= 2 adds homeostasis and the metabolic update; depth >= 3 adds ATP fatigue.
- The neuromod EMA, volume transmission and the LC gate are not depth-gated. The UI slider tooltip ("3: Homeostasis/neuromod") is wrong.
- Auto depth is in `src/distributed.rs:10951-10988`.
- No environmental drivers exist. `external_reward` (`src/bridge.rs:177`) is None from the MuJoCo adapter. Food, light and heat exist only as scene stimuli read by sensors.

Telemetry:
- There are no plots of modulators or synapse energy: the egui UI shows a text line and sliders; the web UI a checkbox.
- `EngineActivity` (`src/engine.rs:85-95`) and `NetworkActivityResponse` (`proto/distributed.proto:277-295`) carry spikes only.
- Synapse "energy" is `Synapse.stimuli` (0..1, `src/morphology.rs:658`). It is aggregated only in an `NM_TRACE` log line (`morphology.rs:8366-8376`).

**Implemented (25 Nov, after the audit).** Neuromodulators now act on transmission in every AARNN network, gated by depth.

Mechanism (`src/aarnn/neuromodulation.rs`, tested; wired in `src/runner.rs`):
- **Levels:** global pools of dopamine (DA), serotonin (5-HT), noradrenaline (NA, new; octopamine/tyramine in invertebrates) and acetylcholine (ACh). Each relaxes toward baseline plus its drives; with nothing driving it, it stays at baseline.
- **Drives** (each a 100 ms population rate, at most 1). Sensory drives are relative to `aarnn_neuromod_sensory_ref_rate_hz` (250 Hz; transducers fire up to ~500 events/s). Motor and modulatory-cell drives are relative to `aarnn_neuromod_ref_rate_hz` (50 Hz). Each pool follows its drive with its own time constant: DA 200 ms, 5-HT 1 s, NA 300 ms, ACh 100 ms.

  | input | modulator |
  |---|---|
  | food sensing (taste, attractant odour) | DA +0.5, 5-HT +0.5 |
  | light | 5-HT +0.5 |
  | energy reserve (satiety / battery) | 5-HT +0.5 |
  | heat (deviation of the thermosensors from their adapted rate, 30 s adaptation as AFD-like cells), noxious input | NA +0.5 each |
  | sensory novelty (change from a 2 s mean) | NA +0.5 |
  | motor activity | DA +0.3 |
  | external reward | DA +1 |
  | the network's own modulatory cells (dopaminergic, serotonergic, noradrenergic/octopaminergic, cholinergic by type or region name; the learners' LC cell) | their pool |

  - Each body's sensory channels are tagged from its IO contract (`aarnn_circuit.sensory_modality`, written as `aarnn_sensory_modalities`). Older snapshots are classified from their connectome channel names at load.
- **Postsynaptic efficacy (depth >= 1):** each cell's chemical synaptic drive, applied after the AMPA/NMDA/GABA filter and before the gap junctions, is multiplied by `1 + rDA (DA - 1) + r5HT (5-HT - 1) + rNA (NA - 1)`, clamped to 0.25-3.
  - Receptor sensitivities are bio params, default DA +0.4 (D1-like), 5-HT -0.25 (5-HT1-like), NA +0.3.
  - At depth 1 every cell uses the network default; from depth 2 each cell type uses its own (D2-like or 5-HT2-like types can set the sign).
  - On the CPU filter path the dynamic ACh level now scales excitability, as the OpenCL filter already did; the CPU/GPU mismatch is closed.
- **Presynaptic release (depth >= 2):** the bouton release probability is multiplied by the same form with DA +0.3, 5-HT -0.3 (5-HT1B autoreceptors), NA +0.2.
- **Depth 0** (the minimal kernel all designed robot circuits use) has no neuromodulation.
  - A no-growth Keenon learner rerun with the new probe reproduces last night's 60 s series exactly: 1200 / 1200 samples identical, both seeds.
- **Learners:** `apply_learner` and the hexapod and NAO learner builders no longer switch neuromodulation off. It has no effect at depth 0; from depth 1, DA and 5-HT scale their learning rate.

Telemetry and graphs:
- **Sample:** `NeuromodSample`, recorded every 50 steps. It holds:
  - the four levels;
  - stimuli (sensory drive);
  - synapse energy (chemical drive delivered per cell, after modulation);
  - mean axon/dendrite ATP and the synapse activity trace when morphology exists;
  - efficacy and release factor;
  - the drives.
- **Where it is published:**
  - `EngineActivity.neuromod`;
  - gRPC `NetworkActivityResponse.neuromod` (`NeuromodState`);
  - web `/api/activity` field `neuromod`.
- **Desktop UI (egui):** a "Neuromodulators" graph at the top right of the network view (checkbox "Neuromodulator graph"), for local, managed and cluster networks.
- **Web UI:** a "Neuromodulators" panel next to the oscilloscope.
- **Simulators:** `run_sim.sh` sets `NM_NEUROMOD_TRACE_DIR` (default `logs/sim_cluster_<pid>/neuromod`), so every MuJoCo, Webots, Unity, Unreal and Isaac run writes `<network>.neuromod.jsonl` beside its physics trace. `scripts/qa/plot_neuromod.py` draws:
  - the levels;
  - stimuli against synapse energy;
  - the drives.
  - No physics viewer draws it live: the physics service has no brain state, and that would need a protocol change.

C. elegans (connectome, depth 5), open loop, 6 s blocks, all sensors at 0 except the block's modality at 0.9 (`scripts/qa/neuromod_stimulus_probe.py`, `target/qa/neuromod/celegans_bbc5`, plot `neuromod.png`):

| block | DA | 5-HT | NA | efficacy | release | hidden rate with / without modulation |
|---|---|---|---|---|---|---|
| baseline | 1.18 | 1.07 | 1.08 | 1.08 | 1.05 | 5.06 / 4.62 Hz (+10 %) |
| food (taste, attractant) | **1.67** | **1.57** | 1.14 | 1.17 | 1.06 | 5.79 / 4.83 Hz (+20 %) |
| light | 1.18 | **1.57** | 1.10 | 0.96 | 0.90 | 4.69 / 4.86 Hz (-3 %) |
| heat | 1.24 | 1.07 | **1.65** | 1.27 | 1.18 | 6.89 / 4.95 Hz (+39 %) |
| noxious (repellents) | 1.18 | 1.12 | **1.69** | 1.25 | 1.16 | 6.74 / 4.92 Hz (+37 %) |

- Food raises DA and 5-HT, light raises 5-HT and damps transmission, heat and noxious input raise NA and arousal.
- The worm's own dopaminergic and serotonergic cells (CEP/ADE/PDE, NSM/ADF/HSN by type) and its motor activity hold DA near 1.18 even at baseline, so the network runs about 10 % more active than before at depth >= 1.
- Motor output was silent in both runs under this open-loop input (all other channels at 0).
- The table above is from the first build: one fast relaxation (about 20 ms per step), absolute heat and a 50 Hz reference for every drive.

First closed-loop check, C. elegans in its MuJoCo habitat, 30 s, frozen anatomy (`target/qa/neuromod/celegans_cl`). The worm sat in the dark at its adapted temperature (heat channels about 0.53), with attractant odour at 0.35-0.38 and occasional taste:
- All pools were pinned high (DA 1.80, 5-HT 1.60, NA 1.80; efficacy 1.41).
  - The 50 Hz reference saturated the food drive.
  - The heat channel's mid-range temperature reading was taken as heat.
- With modulation the worm moved 2.3x as far (path 0.52 m against 0.23 m; net 0.117 against 0.067 m) on 1.7x the muscle activation.

Fixes:
- **Sensory reference:** 250 Hz for sensory drives.
- **Heat:** driven by the thermosensors' deviation from their adapted rate.
- **Dynamics:** per-pool time constants.
- **Telemetry:** raw rates (`rates_hz`) added.

After the fixes, the same 30 s closed-loop worm. Heat now uses the deviation from the temperature the body was born into, adapted over 30 s.

| run | DA | 5-HT | NA | efficacy | release | net displacement | path | mean muscle activation |
|---|---|---|---|---|---|---|---|---|
| modulation off | 1 | 1 | 1 | 1 | 1 | 0.074 m | 0.252 m | 0.142 |
| modulation on (first build, saturated) | 1.80 | 1.60 | 1.80 | 1.41 | 1.22 | 0.117 m | 0.519 m | 0.251 |
| modulation on (calibrated) | 1.34 | 1.21 | 1.23 | 1.15 | 1.09 | 0.164 m | 0.388 m | 0.181 |

- Calibrated drives: food 0.25, falling from 0.45 to 0.2 over the run as the worm moves down the odour gradient; activity 0.36; heat 0.15 (small temperature changes as it moves); novelty 0.05; light 0 (dark habitat).
- Raw rates are in the trace (`rates_hz`): attractant cells ~62 Hz, thermosensors ~280 Hz, dopaminergic / serotonergic / octopaminergic-tyraminergic cells 5-7 Hz.
- Neuromodulation makes this worm more active and travel about twice as far.
- It does not yet steer it toward food: the food drive falls during the run. Learning to use the modulators (food -> DA as the teacher) is the learned-foraging work (F2).
- Plots: `target/qa/neuromod/celegans_cl/neuromod_mod_v3.png` (closed loop), `target/qa/neuromod/celegans_bbc5_v2/neuromod.png` (open loop).
- Desktop UI graph, captured by replaying the calibrated closed-loop worm trace into the UI on Xvfb (glow renderer): `target/qa/neuromod/ui_capture/frames/frame_000009.png` (panel crop `neuromod_panel_crop.png`).
  - The UI snapshot carries the runner's last 64 samples, and the managed/replay path appends every new sample, so the curve stays complete when the UI renders slower than samples arrive.

### Rust trace-replay videos and two new showcase videos (25 Nov)

`aarnn-mujoco --replay TRACE --video OUT.mp4` (`adapters/mujoco/src/replay.rs`, viewer feature) renders a recorded lock-step trace in Rust. It replaces the Python `render_trace_video.py` for new videos.

How it works:
- **Posing:** each frame poses the body at the recorded `qpos` (new `Physics::set_qpos`) and renders through the native viewer at the requested size, on a virtual display (Xvfb).
- **Overlays** use MuJoCo's own text and figures (`mjr_overlay` / `mjr_figure`, added to the bindgen allowlist):
  - title, simulated time, and the energy needs (reserve, refill, caretaker feeding, source);
  - `--neuromod FILE`: the runtime's `NM_NEUROMOD_TRACE`, drawn as DA / 5-HT / NA / ACh in percent above baseline. The brain clock is placed proportionally on the body's time axis.
  - `--plot-sensor NAME,LABEL,A,B`: `A + B x` of a sensor channel;
  - `--plot-near ...,THRESHOLD,WINDOW_S`: the running share of time below a threshold.
- **Output:** raw RGB frames are piped into ffmpeg, with no intermediate images. 1280x720 renders at ~75 frames/s.
- **Range gotcha:** MuJoCo widens a figure's y-range outward to the order of magnitude of its span (0.8-2.2 becomes 0-3), so the renderer picks ranges whose ends are multiples of it.

New videos in `target/showcase-videos/` (README updated; `target/qa/showcase_v4/make_v4.sh neuromod|learning [capture]`):
- **`neuromodulators_celegans.mp4`** (30 s): the C. elegans connectome closed loop with neuromodulation on and off, side by side, plus the Rust UI with its Neuromodulators panel.
  - With modulation: DA about +35 %, 5-HT +20 %, NA +15 to +35 %, efficacy +15 %; the worm travels 0.16 m against 0.07 m.
- **`hexapod_learns_with_growth.mp4`** (300 s at 5x): the hexapod on the fixed gait with growth (seed 7), re-recorded with energy needs. The trajectory is identical to the run without needs.
  - Time within 0.4 m falls 15.4 -> 3.9 -> 0.1 % per 100 s, with three caretaker recharges.
  - Growth-off on the same gait is still to be compared over 600 s pairs.

### Spatial-mode calibration tool, 24 Nov

`scripts/calibrate_spatial_network.py`:
- `gains`: per-synapse route attenuation, delay and release probability, from the runtime;
- `route`: divide each weight by its route gain;
- `compare`: per-cell activity, point cells vs spatial, on a recorded trace;
- `fit`: route correction plus per-cell tonic correction.

Get-up network:
- route gains: excitatory median 1.088, inhibitory 0.989 (routes explain an E/I ratio of ~1.1; the rest is compartment integration);
- uncalibrated spatial vs point cells: mean difference 0.34 Hz, 11 of 304 cells off by more than 2 Hz (sensory relays and traces run 2-3 Hz hotter).

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
