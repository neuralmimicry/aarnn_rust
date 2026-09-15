# Shared simulator habitats and morphology

`catalog.json` and `scripts/sim_content.py` define the reference content for Webots,
Unity, Unreal, browser WebGL and Minecraft. Six neural profiles use five habitats
with 602 authored objects. The engines
consume generated local assets, so scenery no longer needs to be maintained five
times. This is an educational, procedural visual baseline with engineering sensory
proxies. It is not a calibrated animal model or a photorealistic asset collection.

## Evaluation and changes

| Environment | Finding | Upgrade | Remaining evidence |
|---|---|---|---|
| Webots | Independently authored worlds; saved NAO poses; several sensory cues represented by distance devices | Shared standalone and mixed habitats; worm internal landmarks and skin transparency; fly eye facets; fish neuromasts and corrected larval palette; upstream NAO and detailed hexapod hardware retained | Cross-engine transducer calibration and long-run contact stability |
| Unity | Many articulated bodies had colliders without visible meshes; worm and hexapod channel order differed | Shared lit habitats and visible morphology attached to existing articulation; anatomy cutaway; canonical worm muscles and hexapod camera/sonar order | Unity Editor build and play-mode evidence; imported fly/fish mapping migration |
| Unreal | Random independent habitat objects and a separate palette; misleading fish stripe claims | Compiled shared scenery/materials and articulated visual skins; light beacons; cutaway option; corrected worm/hexapod mappings | Render comparison, contact stability and remaining transducer migration |
| Minecraft | No AARNN adapter existed | Fabric 1.21.1 mod and native Bedrock packs, six plots, shared anatomy and WebGL sensor/motion reference, Rust companion JAR, saved worlds, edition detection and BDS port allocation | Bedrock client rendering remains unverified; cuboid tessellation differs; animal flight/fluid/articulated physics remain schematic |
| WebGL | Stick figures and sine-wave sensory values unrelated to scenery | Solid shaded geometry, orbit/zoom/inspection, internal anatomy, spatial fields/proximity and temporal retinal changes | Physical transducers, fluid dynamics and rigid-body execution are outside this kinematic reference |

| Profile | Sensory/output channels | Habitat and relevant content |
|---|---:|---|
| C. elegans | 24 / 96 | Agar, bacterial lawns and colonies, food/thermal field proxies, moist refuge, contact ridges; tapered cuticle, four muscle quadrants, pharyngeal bulbs, intestine, schematic nerve ring and amphid/phasmid landmarks |
| Drosophila BANC | 418 / 48 | Orchard, fruit/bruises, landing stems, leaves/veins, contrasting optic-flow panels; compound-eye facets, antennae/aristae, proboscis, wings/veins, halteres and six legs |
| Drosophila FAFB | 418 / 48 | Same adult fly morphology and habitat; retains separate imported output identities |
| Hexapod | 34 / 18 | Graded steps, gravel, slalom and traction surface; chassis decks, servo housings, feet, sonar transducers, camera and wiring |
| NAO | 250 / 40 | Room, reachable coloured objects, lower table, doorway, steps and target ball; head cameras, shell panels, joint covers, hands and foot pads |
| Zebrafish | 32 / 32 | Shallow freshwater, plants, pebbles, refuge, prey particles, current proxy and surface markers; fins, eyes, opercula and lateral-line neuromasts |

## What parity means here

The shared **object identities, normalized layout, primitive dimensions, colours,
cue definitions and morphology catalogue** have one compiler and digest. Byte-level
freshness tests compare every export and maintained generated robot/world source.
Webots retains its established articulated rigs and supplements their anatomy;
Unity, Unreal, WebGL and Minecraft use the shared visual parts directly. Physical hulls and
visual detail are separate. Additional anatomy adds no motor channels or mass.

Rendering uses each engine's lighting pipeline. Shadows, transparency, raster
resolution, collision shapes, joint solvers, body scale, sensor units and trajectories
are not proven equivalent by the content digest. Native skin attachment currently
uses nearest articulated bodies and requires review in motion. Webots proximity
hulls and browser rays use conservative boxes for curved scenery. Native engines
use primitive colliders. The browser cannot substitute for a rigid-body experiment.

The native load probe exposed large Webots mass ratios and Unreal physics material
initialisation errors. Unreal's NAO also left its room during the unattended probe.
These observations prevent a stability/parity certification; no claim of stable
closed-loop biological behaviour follows from loading the worlds.

## Units and biological interpretation

Authored coordinates are right-handed **X forward, Y left, Z up**. Sizes are full
extents, yaw is radians around Z, habitat coordinates are in units of half extent,
and robot parts use body lengths. Webots defaults use `half_extent_m` in metres.
Unity converts `(x,y,z)` to `(-y,z,x)` and scales scenery around the existing rig;
Unreal converts to `(x,-y,z)` and centimetres. Browser `body_length` and `body_height`
are normalized presentation values. Minecraft converts to `(x,z,-y)` with a
16-block habitat half extent and shares these presentation fractions. Multi-robot layout and native physical rigs can
require different absolute scales; these are not animal dimensions. Webots fish
retains its existing 2.2 m/s² gravity/contact approximation in standalone worlds.
The fish habitat has a water volume of `.625` half extents, above the whole body.
Webots exports a freshwater `Fluid` (1000 kg/m³, viscosity .001 Pa·s) and fish
immersion properties. Unity, Unreal and WebGL draw transparent water with the
same extent; Minecraft fills a glass tank with native water blocks. These water
volumes make submersion explicit without asserting calibrated hydrodynamics.

Field cues evaluate `clamp(sum(strength / (1 + 4*d²/radius²)), 0, 1)` in the habitat
frame. They are dimensionless proxies, with no concentration, temperature or fluid
unit claimed. A moisture refuge is visual/ecological content; no humidity channel
has been invented. Retinal changes are measured from successive views, not a sine
wave. Browser locomotion is a bounded activity-to-motion illustration, not a learned
gait: worm named quadrants exclude MVULVA; fish uses tail pairs; other imported
readouts do not establish a biological muscle-to-joint map.

Anatomy follows conventional adult hermaphrodite nematode and adult fly landmarks,
and larval-inspired fish morphology. These are schematic structures, not fitted
microscopy, a connectome embedding, or a claim that the imported neurons occupy the
displayed positions. C. elegans has no eyes. Fish myomere landmarks must not be
described as adult pigment stripes; adult zebrafish stripes are longitudinal.
Landmark references include [WormAtlas](https://www.wormatlas.org/),
[Virtual Fly Brain](https://www.virtualflybrain.org/) and [ZFIN](https://zfin.org/);
no measurements or fitted parameters were imported from those resources.
Scientific parameter provenance, fitted error bounds and behavioural validation
remain necessary before using these worlds for biological conclusions.

## Channel compatibility still requiring migration

`scripts/robot_profiles.py` owns vector dimensions; Webots `.io_alignment.json`
owns the available ordered names. The generator preserves those names exactly.
The worm's 95 body-wall muscles are grouped by MDL/MDR/MVL/MVR labels; MVL24 is
absent and index 95 is MVULVA. Native/browser code must not reinterpret this vector
as four interleaved muscles per segment. Hexapod camera ON/OFF occupies 30/31 and
left/right sonar 32/33.

Existing fly native channels 0–33 describe joints/feet/antennae, while Webots uses
inertial/touch/proximity channels. Existing fish native channels and fin assignments
also diverge, and Webots chemical/flow devices remain distance/light approximations.
NAO has no equivalent checked-in alignment manifest. These are recorded migration
gaps, not intentional biological variants. Reusing a trained brain across engines
requires the appropriate transducer/motor mapping and calibration; identical vector
lengths or AER transport do not establish equivalent behaviour. No neural kernel,
persisted brain, production I/O gate or model import is changed by the visual build.

## Regeneration and checks

From the repository root, with Python 3, Node.js and the repository Rust toolchain:

```sh
python3 scripts/regenerate_simulator_assets.py
python3 scripts/regenerate_simulator_assets.py --check
cargo xtask qa run --suite simulator-content
cargo test --locked --test web_ui_browser_compat --test run_examples_launcher
```

The visual-only regeneration command leaves network JSON, brain configs and
`.wbproj` editor state untouched. Do not run the full network import generators
merely to refresh scenery. Date-stamped `*.combo-*.wbt` files are historical capture
snapshots; the maintained references are the species worlds, `neuroworld.wbt`,
`multi_neuroworld.wbt` and `multi_neuroworld_test.wbt`. `hexapod_neuroworld.wbt` adds
a standalone terrain reference. The NAO reference starts from a clean default pose.
The old `--fridge on` option is rejected explicitly because shared room furniture is
now authored in the catalogue/compiler rather than a Webots-only switch.

Optional browser evidence uses Playwright installed outside the production app:

```sh
NODE_PATH=/path/to/playwright/node_modules NM_CHROMIUM=/path/to/chromium \
  cargo xtask qa run --suite simulator-content-browser
```

Every xtask lane writes a unique result directory with the scenario/content/fixture
digests, Git revision/dirty state, toolchains, capabilities, elapsed time, child
CPU/RSS metrics and pass/fail logs. Requested missing capabilities fail the lane.
The browser lane starts its own loopback asset server, mocks the gateway, checks all six profiles,
disconnect/stale-response/timeout handling and mobile layout, then saves screenshots
and a report under `target/qa/simulator-content/`. It does not execute a brain.
The installed-engine Webots construction lane runs with
`cargo xtask qa run --suite simulator-content-webots` (requires a working Webots display
stack); it creates an isolated temporary project with all neural controllers
disabled, checks a supervisor-written report and retains the engine log.
Engine build commands and launchers are in [the simulator guide](../README.md).
Unity's manager builds shared habitats by default; disable `buildSharedHabitats`
when supplying an authored scene. Unity's `NmRobotAppearance.anatomyCutaway`,
Unreal's game-mode `bAnatomyCutaway`, Webots worm `skinTransparency`, and the browser's
Anatomy cutaway control expose internal landmarks without altering collision rigs.

See [the living execution plan](../../docs/execplans/simulator-content-parity.md)
for exact validation results and unresolved native gates.

Minecraft build, installation, saved world, per-profile checks and limitations are
in [the Minecraft guide](../minecraft/README.md). Its loopback companion uses
the existing Rust sandbox protocol and does not promote production I/O gates.
