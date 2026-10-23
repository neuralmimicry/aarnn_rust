# Model and adapter source notices

The supplied source packs remain immutable. Generated MJCF, USD, rigid graphs and Webots files
are derived artefacts, not independent OEM designs or evidence of task success.

| Source | Pinned revision | Licence / scope |
| --- | --- | --- |
| Galaxy General Robotics `galbot_one_golf_description` | `2d496b053f0d4e9e2688f59fac66022f447226be` | Apache-2.0; official Golf meshes, MJCF and USD. See [licence](licenses/Galbot-Apache-2.0.txt) and original `.agent/robot-expansion/assets/galbot-g1-golf/LICENSE`. Generated files preserve names but include adapter/world changes. Exact LEC variant/calibration remains pending. |
| dnorton94 `ugot-robot-robomasters` | `ef935a3f22a179ae815e1d0c5108a082de1d0432` | MIT; community estimated mecanum mechanics. See [original notice and licence](licenses/UGOT-MIT.txt). This is not OEM measured CAD. |
| Original reduced commercial/crow/legacy mechanics | Source/compiler hashes in generated records | Authored in this repository. No root repository LICENSE was found; no new distribution licence is invented here. Scientific facts and the disclosed assumptions are separate from third-party geometry. |
| Species facts and published commercial envelopes | Pinned task source catalogue and manifests | References, not a grant for copying arbitrary vendor CAD or brochure artwork. No such artwork is copied into these generated models. |

No optional BlendSwap crow mesh, BIRD_SIM source, unlicensed NAO mesh or gated
asset was incorporated. The six new native biological bodies use original
primitives; retaining their neural names does not prove controller transfer.
The bundled OEM SDK wheel was inspected as a source reference, not installed as
a robot driver or treated as a complete design.

Habitat and biological illustration additions are original procedural repository
source, reusing `scripts/sim_content.py` and its catalogue. No external artistic
mesh, texture or image was acquired. Worm v2 cuticle/annuli/internal landmarks,
fly/fish schematic anatomy and crow feather details are engineering illustrations,
not scanned anatomy. Their source and assumption hashes are retained in the
generated physical records; the existing Golf/UGOT asset licences remain unchanged.

Legacy native neural adapters port facts and numerical mappings from this
repository's DeviceMapper, motor-response helper, controller bridges and original
PROTO/alignment sources. No new OEM mesh or executable driver was acquired.
The proposed BANC/FAFB motor readouts are separately versioned engineering
assumptions; imported neuronal IDs are not inferred anatomical innervation.
Shared Rust optical cells, passive water forces and servo limits add no new
third-party runtime dependency. See `legacy-native-v1.json` for provenance and
calibration limits; the unresolved root repository distribution licence remains
unchanged.

Native engine/dependency boundaries:

- MuJoCo 3.17.0 uses its separately installed Apache-2.0 SDK and bundled third-party
  notices. The repository does not vendor a new SDK binary. `bindgen` generates
  bindings from that exact SDK, rather than linking incompatible `mujoco-rs`
  structs. Its lock entry is `bindgen 0.73.2` (BSD-3-Clause).
- Webots R2025a uses its installed controller SDK/library and engine licensing;
  C API headers are Apache-2.0. No Webots engine binary is copied into source.
- `signal-hook 0.3.18` is MIT OR Apache-2.0. Shared serde/serde_json/sha2 and
  axum/tokio remain Cargo.lock-pinned; their upstream package notices apply.
- The optional native Rust viewer loads the system GLFW 3 runtime (zlib/libpng
  licence), through `libloading 0.8.9` (ISC). Neither GLFW nor an OpenGL driver
  is vendored. Headless native/service and portable crates require no window
  runtime. The C. elegans muscle readout ports this repository's existing C++
  response v2; original network/configuration and Webots sources are retained.
- Python MuJoCo/numpy and viewer dependencies are pinned separately in
  `sim/mujoco/reference-requirements.lock`. `usd-core 26.5` is the offline USD
  converter in a different Python environment; its OpenUSD upstream licence
  and bundled notices apply. It neither installs nor licenses Isaac Sim.
- Unreal Chaos uses the user-installed Unreal Engine 5.8.0 (CL 55116800) and its Epic licence. No engine binary is vendored. Unity uses a separately licensed installed Editor; its native factory is source-only until that runtime is available.
- Minecraft Java/Fabric, Bedrock, Mojang/Microsoft server runtimes and official API packages retain their own licences/terms. Game binaries and private server secrets are not added to source.
- Isaac Sim runtime, PhysX schema extensions and engine distribution remain
  separate target requirements. A parseable USD does not certify that runtime.

Regeneration verifies all 467 original bundled asset digests before using them.
Do not alter an upstream file to make a generated-content freshness check pass.
