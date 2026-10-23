# AARNN: LEC robots and biological crow agent pack

Prepared for Paul B. Isaac’s · 2026-10-23 · British English

This pack contains the research, licensed design sources and implementation instructions for **15 LEC commercial model families, expanded to 21 configurations, plus a carrion crow: 22 new profiles**. It also includes instructions to port **all six existing AARNN profiles** into the new MuJoCo and NVIDIA Isaac Sim backends: **28 profiles across eight runtime targets in seven simulator families**, counting Minecraft Java and Bedrock separately. The full target set is **Webots, Unity, Unreal, browser WebGL, Minecraft Java, Minecraft Bedrock, MuJoCo and Isaac Sim**. This is an implementation hand-off for the Codex interface in RustRover, not a claim that the target expansion already runs in AARNN.

## Start in RustRover

1. Extract this archive. Open `/home/pbisaacs/Developer/neuralmimicry/aarnn_rust` as the RustRover project root.
2. From the extracted pack, run the installer below. It copies this pack into `.agent/robot-expansion/`; it does not edit the root `AGENTS.md`, simulator code, a brain or an existing network.
3. Paste the complete text of `START_HERE.md` into the Codex interface. The prompt explicitly loads the task instructions; nested `AGENTS.md` files in this pack do not automatically govern unrelated `src/` or `sim/` directories.

```bash
python3 tools/install.py --repo /home/pbisaacs/Developer/neuralmimicry/aarnn_rust
python3 tools/install.py --repo /home/pbisaacs/Developer/neuralmimicry/aarnn_rust --apply
```

The first command previews the copy. The second performs the reversible additive install. Different existing destination files cause refusal; identical files are safe to reuse. Keep any existing repository instructions.

## What is ready

- `AGENTS.md`, `agents/` and 28 `robots/*/AGENTS.md` task files, plus a phase-compatible ExecPlan template.
- Official Apache-2.0 Galbot One Golf URDF/MJCF/USD/Xacro and dependent meshes, at `2d496b053f0d4e9e2688f59fac66022f447226be`. The official developer platform lists these as G1 description assets; the exact LEC G1 configuration remains unconfirmed.
- MIT community UGOT mecanum MJCF/USD, parameter ledger and generator at `ef935a3f22a179ae815e1d0c5108a082de1d0432`. It has estimated geometry and inertias, and is not official UBTECH CAD.
- MIT UGOT Python SDK 0.2.13 wheel, as optional API reference. It is not installed or executed.
- Source and checksum records, known specification conflicts, a proposed 492-input/64-output crow transducer map, and 224 explicitly unrun target/profile acceptance rows.

## What still needs implementation or vendor data

No verified open complete mechanical model was found for the eight KEENON products, BOTINKIT OMNI, the two XBOT products, the two COFE+ products, or the six other UGOT forms. Their agent files provide original parametric-surrogate routes and exact-model upgrade requirements. Brochures, a public SDK and a 3D animation are different kinds of evidence.

The CC0 crow Blender model is identified at [BlendSwap](https://blendswap.com/blend/7340), but its download requires sign-in. It is optional visual material. The crow agent must create an articulated physical surrogate with explicit parameter provenance; no validated crow connectome or complete musculoskeletal model was found. An original procedural visual fallback avoids making progress depend on that download.

**Full functionality in WebGL and Minecraft requires the physics-service work in `spec/physics-and-engines.md`.** Their current AARNN bodies are kinematic previews. A pose animation alone cannot pass the final task gate.

## Read next

- `research/inventory.md`: complete brands/models and design availability.
- `research/repository-map.md`: inspected AARNN paths and integration gaps.
- `spec/requirements.md`, `spec/aer-contract.md`, `spec/crow-biology.md`: implementation contracts.
- `spec/acceptance.md`: evidence required before calling a robot functional.
- `EXECPLAN.md`: plan to adapt under the repository’s existing phase gates.

## Verify this pack

```bash
python3 tools/validate_pack.py
python3 -m unittest discover -s tests -v
python3 tools/repo_audit.py --repo /home/pbisaacs/Developer/neuralmimicry/aarnn_rust
```

The first two verify this deliverable’s structure and helpers. The third reads local repository state. None proves simulator dynamics or neural behaviour. `tools/fetch_assets.py --group galbot` restores missing pinned source files if needed and verifies SHA-256 without executing downloaded content. Bundled assets already have the expected bytes.

Source inspection used AARNN `a02c9c43089b5cb488094b50124d034ae51b4ca3` (2026-10-13T23:55:25Z). The local checkout may be newer or contain uncommitted work: the first agent step is discovery and reconciliation, without reset or replacement.

## Verification delivered

Pack integrity, all 467 pinned source files, authored Python syntax and four focused helper tests pass. Static inspection resolves the bundled Galbot URDF mesh dependencies and local USD assets; its OmniPBR material module still depends on the Isaac/Omniverse runtime. The two literal AER1 goldens agree with the inspected AARNN Python compatibility codec. These checks have not executed a simulator or neural network. See `verification/pack-validation.json` and `verification/model-source-inspection.json`.
