# Minecraft verification record — 15 September 2026

The shared catalogue contains **602 objects, five habitats and six robot/network
profiles**, digest `4d0d506e80f146acf04e34ee781f67ab465c6d1f336b73c799bc3e3904a6b2d8`.
Java and native Bedrock adapters use the same Rust companion. No Java or
JavaScript neural executor was added.

| Check | Evidence and result |
|---|---|
| Shared assets | Six content tests, spatial sensor fixtures and regeneration checks pass |
| Java/Fabric | 13 JVM tests pass; native GameTest constructs all six profiles, verifies actual water, native block/food sensing, persistence and disarming, then exits cleanly |
| Java saved world | Cropped region export passes; source/report/save hashes bind clean-exit acceptance to the distributed ZIP |
| Java rendering | Native Minecraft loads the exported world with exactly 12 entities and captures 12 habitat/cutaway views; no leftover test terrain; zebrafish submerged |
| Bedrock packs | Six profiles match the sensory oracle; geometry, native-water construction and session/permission/failure behaviour pass controlled API fixtures; Microsoft API type checks pass |
| Detection/launch | Four Java and six Bedrock detection cases plus four real-socket/lock tests pass; occupied BDS TCP/IPv4-UDP/IPv6-UDP ports are reassigned; native managed launch preserves original properties and exits cleanly; headless companion authentication/cleanup passes |
| Browser | All six profiles render with anatomy and transparent water; cancellation/timeout/mobile-layout checks pass |
| Webots | All six robots and 602 habitat objects load, including freshwater Fluid and fish immersion properties |
| Unreal | Editor target builds; a native fish/water framebuffer was captured. This is rendering evidence, not a clean-exit or stability certification |
| Unity | Not run: Unity Editor unavailable |
| Native Bedrock runtime | BDS 1.26.45.1 passes six real Rust network connections, 12 native entities/18,992 blocks, fish submersion, disarming, robot and vanilla NPC identity before/after reload, occupied TCP and IPv4/IPv6 UDP reassignment, and clean shutdown |
| Bedrock client rendering | Not run: a compatible Bedrock graphical client is unavailable; server validation is separate from visual acceptance |
| NAO social model | Ten behaviour/model tests and nine real Rust-selected acts pass, including an autonomous encounter inquiry; original 250/40 matrices and labels preserved in a separate 282/49 reference model |
| NAO browser | Real Chromium and Rust pass text, inquiry, bubble, cancellation, stop/rejoin and layout; speech devices are controlled fixtures |
| NAO native body integration | Webots and Unreal pass native sensors/body transport and Rust/chat HTTP; native-window clicks and microphones are not verified |
| NAO native Bedrock inquiry | Installed exported world, actual vanilla villager, native encounter scan, companion JAR and Rust produce the question in NAO's name tag; clean shutdown passes |
| Participant senses | All six Java profiles pass native NPC sensing; shared browser/Bedrock sensor fixtures preserve existing species modalities; non-speaking species gain no text receptors |

Every network below loaded its real imported snapshot into an isolated Rust
process and completed **16** bounded sensor/output round trips through the
companion. Snapshots were not modified.

| Profile | Input/output channels | Returned spikes | Maximum round trip in accepted run |
|---|---:|---:|---:|
| C. elegans | 24 / 96 | 9 | 0.231 s |
| Drosophila BANC | 418 / 48 | 144 | 14.917 s |
| Drosophila FAFB | 418 / 48 | 144 | 22.381 s |
| Hexapod | 34 / 18 | 54 | 0.248 s |
| NAO | 250 / 40 | 120 | 0.142 s |
| Zebrafish | 32 / 32 | 93 | 0.996 s |

The first BANC run exhausted the former 30-second deadline under concurrent load.
Its focused rerun passed with the final 60-second fly budget. The other profiles
passed in the six-profile run; zebrafish passed again after its depth sensor was
aligned with the authored water volume. These numbers describe these short
sandbox runs, not sustained performance or biological adequacy.

The additional native BDS run completed **four frames per profile** using the
actual starting habitat pose. It returned 18 hexapod spikes and zero spikes from
the other five profiles in that short interval. This verifies real native
sensor/transport/output handling; it does not imply that each brain moved or
demonstrated a biological behaviour. The native world export uses the saved,
disarmed lab and embeds the offline packs, with no QA observer or server secret.

Retained repository evidence:

- `target/qa/minecraft/contract-17h5isxy/`: JVM build and reference checks.
- `target/qa/minecraft/world-tlmb7e_5/`: final native world with all-six NPC sensing, first-neural-output readiness assertions, clean exit and refreshed JAR/world build.
- `target/qa/minecraft/visual-juva7meh/`: 12 native screenshots and saved-world reload.
- `target/qa/minecraft/neural-zy_qesq0/`: five accepted profiles and the explicit initial BANC failure.
- `target/qa/minecraft/neural-qpja_is2/`: accepted BANC rerun, with snapshot/output hashes.
- `target/qa/minecraft/neural-9v3yxg9k/`: final zebrafish depth/real-snapshot round trips.
- `target/qa/minecraft/bedrock-p3dzqchb/`: final Bedrock pack/API fixtures,
  ordinary-player commands and permissions, offset-timer and delayed-first-frame
  NPC regressions, metadata preservation/malformed-input tests, detection,
  real-socket tests and official type checks.
- `target/qa/minecraft/bedrock-native-82l2875w/`: final scripts on BDS 1.26.45.1, all six real Rust
  snapshots, occupied TCP and RakNet IPv4/IPv6 ports, native water, stop/reload,
  identical saved robot IDs, correct vanilla NPC identity, clean exit and certified
  `.mcworld` export. Full native metadata supersedes the earlier development exports.
  The complete lane took 386.5 seconds; each network returned four frames.
- `target/qa/nao-social/run-mh9lcfm_/`: nine real Rust speech acts, including inquiry.
- `target/qa/nao-social/run-ds9yews2/`: final Chromium/Rust interaction and layout.
- `target/qa/nao-social/webots-elfe1rdw/` and `unreal-mvj6w6th/`: native body/Rust/chat
  round trips. `target/nao-social/unreal-final-build.log` records the UE 5.8 build.
- `target/qa/nao-social/bedrock-c3ua3jl_/`: actual native NPC/Rust inquiry and
  name-tag bubble in a fresh installation of the final exported world; clean
  shutdown. Its recorded world SHA-256 matches the packaged native certificate.
- `target/qa/minecraft/bedrock-launch-vcwu2c0c/`: actual managed launcher, automatic
  TCP reassignment, unchanged original properties, console forwarding and clean exit.
- `target/qa/minecraft/bedrock-reload-jop9s1xa/`: focused saved-entity preload check.
- `target/qa/minecraft/bedrock-export-n_eiqsf5/`: distributed `.mcworld` extracted
  into a fresh server using the documented overlay procedure; all 12 entities,
  native water and disarming verified again, followed by clean shutdown.
- `target/qa/simulator-content/browser-m1nxueil/`: final browser rendering and lifecycle checks.
- `target/qa/simulator-content/webots-r1e_rg82/`: native Webots construction.
- `/tmp/aarnn-minecraft-headless-result.log`: actual headless launcher, authentication and cleanup.
- `/tmp/aarnn-minecraft-water-unreal-build.log`, `/tmp/aarnn-unreal-water-final.png`:
  Unreal build and fish scene observation; the bounded render process was terminated after capture.

`SHA256SUMS` in the distribution identifies delivered artefacts. QA result JSON
records toolchains, platform, catalogue/scenario digests and artefact hashes.
Native screenshots are retained locally under the visual evidence directory.
`bedrock-world-acceptance.json` binds the native `.mcworld` to its clean-exit
report and saved/generated source hashes. Initial native failures remain in the
earlier `bedrock-native-*` bundles; corrected final acceptance is listed above.

The extra native NPC test exposed incomplete metadata in the earlier development
export: a vanilla villager could report a habitat identity despite successful
robot-only checks. The refreshed world is bootstrapped by BDS itself, preserving
native version/storage fields. Export now requires vanilla NPC identity on both
construction and reload. The encounter scan also uses elapsed ticks so an offset
four-tick callback cannot skip every scan. These are distinct from language ability:
NAO's nine-act circuit is engineered, and improved learning/general understanding
has not been demonstrated. See [NAO installation and interaction](../nao/README.md).

The real native encounter test additionally exposed an idle-startup race. Both
Minecraft adapters now wait for a validated neural frame before starting an
encounter, so merely arming a body cannot consume the one-time invitation before
the social service is receiving frames. The initial failure and its request trace
remain in `bedrock-ah615am8/` and `bedrock-o9c2xsv4/` under `target/qa/nao-social/`.

Content/transducer agreement does not establish identical rendering, hydrodynamics,
contact physics, neural trajectories or biological behaviour across engines.
Bedrock uses cuboid approximations; existing native fly/fish channel differences,
Webots mass-ratio warnings and native stability/calibration gaps remain described
in `sim/content/README.md`. This is an opt-in legacy AER1 sandbox; production
workstation I/O and its clock mapping, admission, commit and EffectId gates remain
unchanged.
