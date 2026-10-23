# Task instructions: LEC robots and biological crow

These are additive task instructions, explicitly loaded by START_HERE.md. They do not replace the repository root AGENTS.md or extend their own filesystem scope into src/ or sim/. The implementation agent must explicitly read the referenced task documents and applicable per-profile instructions.

## Outcome and authority

Implement 22 new selectable profiles: 21 configurations corresponding to 15 LEC commercial families, plus Corvus corone. Port the six existing profiles into the added MuJoCo and Isaac Sim backends and preserve all prior targets. The complete matrix is 28 profiles across eight runtime targets in seven simulator families. Each instantiated body must sense its own environment, send admitted sensory events to its own selected AARNN network and apply that network’s authorised output through its own motor transducers. Count instances, brains, workers and renderers separately.

Existing root instructions, normative distributed-emulator semantics, applicable ADRs and active phase gates remain authoritative. Create a living ExecPlan using the repository format. Keep the new integration additive and disabled at production boundaries until their existing prerequisites pass. Do not copy Rust sources into a second workspace or create a second neural runtime.

## Execution rules

1. Discover current HEAD, working changes, instructions, profiles, content generators, launchers, AER gateways and QA orchestration before editing. The research snapshot is a starting map, not a replacement checkout.
2. Read agents/00-discovery.md through agents/60-validation.md in order; engine files are separate sequential work packages. Read each robots/<id>/AGENTS.md explicitly.
3. Keep immutable upstream source assets and licence notices under the installed pack or the repository’s established third-party asset location. Generated imports belong to the chosen generator, not hand-edited engine copies.
4. Derive one versioned ordered IO contract from each selected physical model. Labels, units, signs, ranges, rates, address maps and digests must accompany counts. Engine-specific ordering cannot silently change a trained network’s meaning.
5. First complete one licensed model through canonical import -> physics -> sensory/actuator response -> AER -> live neural task. Then reuse the mechanism for delivery, cleaning, kiosk, UGOT and bird archetypes. Do not stop after a catalogue entry or visual-only robot.
6. Complete original functional surrogates when OEM data is missing. Record each unknown/assumption and keep exact OEM reproduction pending. No fabricated download URLs, model equivalence, vendor motor IDs, simulator results or learning claims.
7. Preserve independent asynchronous neural progression, non-blocking rendering and slow morphology work. No fleet-wide barrier on every tick, no conversion of packet-arrival time into biological time, no silent dropping of committed events.
8. Only permitted committed output may produce effects. Separate diagnostic controllers, recorded playback and neural control in configuration and evidence. Neural-silence tests must remove active drive; passive momentum and falling remain physical.
9. Record file/asset revisions, generated-content and mapping hashes, configuration, seeds, feature profiles, exact commands and runtime evidence. Missing capabilities are not passing tests.

## Scope boundaries

The user authorises simulation source work, model import, software AER transducers and neural connections inside AARNN. This is not authority to contact vendors, buy/download gated assets through an account, modify firmware, operate physical robots, charge cards or control real building systems. Prepare the exact missing-asset request in research/vendor-data-needed.md; do not send it.

## Completion

All requirements and model/target rows must be resolved or explicitly blocked with evidence. Keep exact-replica, mechanical, AER and neural-behaviour acceptance separate. Deliver launchable functional surrogates for available targets and do not call the complete expansion finished merely because all source files compile.
