# Functional robot expansion requirements

## Scope and fidelity

REQ-001: Add the 22 new profiles in `manifests/robots.json`; retain all six existing profiles and port them to MuJoCo and Isaac Sim. The full regression/acceptance matrix has 28 profiles × 8 targets = 224 rows. “All robots” includes distinct Drosophila BANC and FAFB; neither is an alias for the other.

REQ-002: Separate three labels: `official-description` (with exact variant status), `functional-surrogate` (published envelope plus explicit engineering assumptions), and `visual-preview`. Passing functional acceptance is permitted for a documented surrogate; claiming exact OEM reproduction requires matching licensed mechanics and calibration. A static render or animation cannot pass the functional gate.

REQ-003: Produce real native physics assets for Webots, Unity, Unreal, MuJoCo and Isaac Sim. WebGL and both Minecraft editions must offer functional mode backed by one authoritative physics service. The existing visual preview can remain, with an explicit mode label.

## Canonical embodiment description

REQ-010: Extend existing content/model schemas with a versioned, engine-neutral embodiment record: stable model/variant IDs; source/licence/digests; SI units and coordinate frame; links; joint transforms/axes/types/limits; mass/COM/inertia; collision shapes/materials; visual LODs; actuators/control modes; passive/mimic constraints; sensors; ports; tools; service interfaces and model uncertainty. Use existing cohesive modules rather than a second Cargo workspace.

REQ-011: Preserve imported vendor names and immutable original bytes. Authored surrogate geometry and every assumed dynamic parameter have a status and rationale. Positive masses, physical inertia tensors, valid rigid transforms and finite dimensions are required. Do not duplicate mass into both a render mesh and a physical body.

REQ-012: Canonical coordinates are right-handed X-forward, Y-left, Z-up; metres, kilograms, seconds, radians, newtons and newton-metres. Store full dimensions rather than silently mixing half-extents. Export vector, quaternion, angular velocity, force/torque and inertia through tested basis transforms. Rendering zoom and display scale must not silently change physical geometry or dynamics.

REQ-013: Generate target assets and ordered IO mappings from one authored source. Generated content has schema/compiler/source/mapping hashes and a freshness check. URDF alone does not preserve sensors, actuators, tendons, closed chains or custom aerodynamics: explicit target emitters and sidecar semantics must restore them.

## Sensors and actuators

REQ-020: A sensor channel names a real or explicitly simulated measurement, mounting frame, units, normalisation, valid range, sample rate, latency/noise profile, timestamp and missing-data behaviour. Sensor values must change with scene geometry, contacts or body state. Sinusoidal placeholders and privileged task-answer/object-identity channels are forbidden in evaluated control.

REQ-021: Each actuator names a physical joint/device, control mode, signs, units, bounds, neutral/fault policy, filter/integrator state and safety interlock. Binary spikes cannot directly encode a signed real torque: use versioned antagonist/rate transducers or another declared encoding. Passive joints and mimic fingers are not independently commanded motors.

REQ-022: Simulate task affordances as stateful physical/process systems: carried payloads, locks/doors, docking, dirt/tank capacity, cups/grips/pouring, food and water, bird toe contact and perch loads. Process approximations must conserve their declared mass/volume/energy quantities. A simulated lift or payment terminal never calls a real service.

## Networks and runtime

REQ-030: Each robot instance has a stable instance ID and an explicit brain binding, model/IO/mapping generations, endpoint, stream and reset epoch. Multiple instances may use the same model but must not share mutable runtime buffers or output authority accidentally. Existing trained networks are never resized silently to accept a new profile.

REQ-031: Reuse the authoritative AARNN runtime, admission and committed-output path. Add software sensor->AER and AER->actuator transducers, not an assumption that OEM robots expose native AER hardware. Legacy AER1 compatibility and governed production AER are separate capability profiles with separate evidence.

REQ-032: Keep sensor capture, neural admission/execution, body physics, growth and rendering independently scheduled through bounded queues. Never hold a render or physics lock across a network request. The common world can advance without a viewer; independent brains need not complete an equal number of cycles. Define bounded stale-output/late-input policies per controller and preserve committed event logs.

REQ-033: Use simulation time and versioned mappings for neural event times; wall-clock time is pacing/latency metadata only. No speculative neural output is treated as a committed motor effect. Preserve output quality, fences, dedupe and reset semantics. Physics state and transducer filter state must be included in an explicitly versioned session snapshot when deterministic closed-loop replay is claimed.

## Delivery and user controls

REQ-040: Extend the unified launcher and GUI selectors with the discovered backend names `mujoco` and `isaacsim` and all profile IDs. Produce headless, interactive and record/replay paths, deterministic seed/configuration selection, readiness/error messages and clean termination. Proposed launch syntax is documented only after implementation passes its tests.

REQ-041: A dashboard shows selected model/variant/fidelity, target physics authority, brain identity, mapping version, capture/actuation state, latency/queue pressure, energy and relevant task state. It supports pause, step, reset, disconnect, disarm and view-only attach. Resetting a body is not permission to reset a remote brain.

REQ-042: Publish per-profile canonical models, engine import/generation recipes, IO manifests, network build/config recipes and launch presets. Include initial sensorimotor networks and a bounded training/evaluation workflow. Untrained networks are labelled; successful mechanics do not prove learned behaviour.

## Existing biological profiles on new backends

REQ-050: Preserve the existing six profile dimensions and named meanings. C. elegans uses its segmented body and muscle mapping; fly BANC/FAFB use separate imported identity sets; hexapod has 18 articulated motors; NAO retains its named joints/sensors; zebrafish needs a declared buoyancy/drag/muscle model. Port the existing approximations with explicit limitations and fix ordered-mapping discrepancies before claiming controller transfer.

REQ-051: Do not invent an open licence for existing third-party geometry (including upstream NAO). Record provenance and reuse only assets the project is permitted to redistribute. A licence blocker does not authorise substituting an unrelated commercial model under the original name.
