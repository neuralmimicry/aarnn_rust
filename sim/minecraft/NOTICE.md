# Provenance and third-party software

The adapter's Java code is authored for AARNN. Shared habitat and anatomical
geometry comes from `scripts/sim_content.py` and `sim/content/catalog.json`.
It is schematic educational content, not a reconstructed connectome.

Design inspiration: Blendi Remade / fal.ai's MIT-licensed
`fly-brain-minecraft` (Minecraft 1.21.1 Fabric), inspected locally on
15 September 2026. The separate mod's Java neural integrator, model textures
and connectome dataset are not included in this distribution.

Gradle wrapper files are the repository's existing Gradle 9.1.0 wrapper,
Copyright Gradle contributors, Apache License 2.0:
https://www.apache.org/licenses/LICENSE-2.0.
Fabric Loader and Fabric API are separate Apache-2.0 dependencies.
The companion JAR bundles Gson 2.10.1 (Google, Apache License 2.0); its upstream
licence and notices are retained in the dependency JAR contents.
Minecraft and Mojang's official mappings are separate proprietary dependencies;
the Minecraft game, mappings and Fabric API are not embedded in the AARNN JAR.
Minecraft installation requires the user's own licensed copy and acceptance of
Mojang's applicable terms. This project is not affiliated with Mojang/Microsoft.
# Third-party distribution notices

The companion embeds Gson 2.10.1, Copyright 2008 Google Inc., licensed under
Apache License 2.0 (`licenses/Apache-2.0.txt`). Fabric API and the official
Fabric installer retain their upstream licences inside the unmodified JARs.
The Gradle wrapper is Apache-2.0 licensed. Minecraft game binaries and Mojang
mappings are downloaded only by the development toolchain and are not included
in the distribution bundle. Minecraft is a trademark of Mojang/Microsoft;
this independent adapter is not an official Minecraft product.

Bedrock behaviour/resource packs and their procedural texture palette are authored
for this repository. Microsoft Minecraft Script API declarations and TypeScript
are build-time verification dependencies pinned in `bedrock/package-lock.json`;
they are not embedded in the add-ons. No Bedrock server binary or proprietary
world database is redistributed. Bedrock networking/admin modules are provided
by the user's compatible Dedicated Server installation.
