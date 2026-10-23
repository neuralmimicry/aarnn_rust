# Physics, engines and full-function frontends

## Common authority

Maintain one canonical physical/model description and one versioned IO/transducer contract. Each world has exactly one active physics owner. The AARNN neural runtime remains authoritative for committed neural output; it is not replaced by a simulator controller. Offline model conversion and online stepping have separate processes and resource budgets.

Webots, Unity, Unreal, MuJoCo and Isaac Sim must load articulated collision bodies, physical constraints, real sensor measurements and declared actuator dynamics. Unit transforms, solver/time-step/material/drive settings and model uncertainty are part of a reproducible engine profile. Contact, friction and locomotion will differ numerically between engines; evaluate declared tolerances and tasks rather than promising identical trajectories.

## MuJoCo: new native backend

Proposed location `sim/mujoco/`, subject to repository discovery. Provide native MJCF for all 28 profiles. Prefer the bundled vendor/community MJCF where applicable; otherwise generate from canonical mechanics with explicit actuators, sensors, constraints and aerodynamic/hydrodynamic hooks. Standalone URDF import is insufficient for these extra semantics.

Use the official MuJoCo API for model/data ownership, joint-name/index resolution, `sensordata`, state, `ctrl` and applied forces. Resolve qpos/qvel/actuator indices by names; nq, nv and nu need not match, especially with free roots, ball joints and passive rollers. Advance physics with `mj_step` under one owner; the optional passive viewer syncs state without becoming another stepper. No blocking socket read inside a control callback. Model reload/reset invalidates indices and clears transducer history/epochs.

Keep a conventional CPU stepping baseline before MJX/Warp/batched acceleration. Use separate data and buffers per world and robot, bounded asynchronous AER queues and explicit input/output step bookkeeping. Select EGL/OSMesa/native rendering only after capability checks; numerical headless operation can run without a viewer, while camera-sensor acceptance requires a working renderer.

Research observed PyPI MuJoCo 3.17.0 and its 22 October 2026 changelog. This is a candidate test baseline, not an unconditional upgrade instruction: pin the installed tested version and dependency hashes, check compatibility with source models and record any migration.

## Isaac Sim: new native backend

Proposed location `sim/isaacsim/`. Produce versioned local USD articulations and scenes for all 28 profiles. The bundled Galbot USD is an input, not evidence that it already loads correctly in the user’s Isaac version. Import URDF/MJCF where useful, then validate units, up-axis, articulation root, root mobility, mass/COM/inertia, collision decomposition, mimic/passive joints, drive mode and sensor frames. No universal automatic converter preserves every source semantic.

Run the backend with Isaac Sim’s supported runtime and Python environment. Bootstrap Kit before importing extensions that require it, select APIs from that installed version and initialise/reset articulations at the documented lifecycle points. The inspected current docs use `isaacsim.core.experimental.prims.Articulation`; older code using `omni.isaac.*` must not be copied without version validation. Resolve DOFs by names and explicitly choose position, velocity or effort control with compatible drive gains. Physics callbacks exchange bounded local buffers; AER networking runs outside those callbacks. Reacquire views/handles after stage reset.

Pin the application release, Python/Kit/USD/physics API versions, GPU/driver and rendering settings. Native physics, RTX camera/lidar and headless offscreen rendering require distinct checks. Run NVIDIA’s compatibility checker and the matching release’s hardware policy; do not assume a generic ARM server or a GPU with adequate CUDA support is a supported Isaac host. Isaac Lab is optional training infrastructure, not a requirement for an AER bridge. Do not bundle NVIDIA’s runtime or remote proprietary assets with this pack.

A single Kit process may host multiple robots only with independent state/bindings and bounded resource measurements. It must not impose a global neural barrier or let one late brain block others. ROS 2 remains an optional adapter; the direct AER client is sufficient for the core task.

## Webots, Unity and Unreal

Webots: generated PROTO/worlds and controller device maps; preserve physical vs visual geometry, motor limits and sensors. Unity: physics construction/prefabs plus explicit spawn factory, background AER and FixedUpdate drive application; avoid hidden control-mode conflicts. Unreal: class/factory registration, articulated constraints, collision and thread-safe AER exchange; use SI-to-centimetre transforms and avoid the existing illustrative animal scale boosts for new models.

All three need both headless construction/physics and rendered interaction evidence. A native build is not an executed neural-control test.

## Browser and Minecraft functional mode

Current WebGL/Minecraft adapters are previews. Implement an authoritative physics-service mode for functional experiments, initially using a headless MuJoCo world (Webots or Isaac Sim can be an explicitly selected alternative). Physics, sensors, contacts, airflow/process state and motor application occur once in that service. Frontends render timestamped poses and send bounded permitted environmental interactions; they do not run competing local body physics or duplicate the AER sensory stream.

Snapshot metadata includes world/model/IO hashes, physics time/step, pose and joint state, robot/brain/stream/reset identities and quality. Interpolation is display-only. Validate stale/reset/versioned messages and constrain interaction commands. Share model geometry and scene manifests while keeping state epochs distinct.

Minecraft Java and Bedrock need explicit coordinate/unit mappings, custom body representation, persistent headless server support and tested world-collision mirrors. Player/block/door/perch changes become versioned physics obstacles/contacts; the authoritative body’s sensors must see them. Disable competing vanilla entity AI/gravity where the service owns the body. Handle delayed collision updates with a documented safe boundary. A smooth visual bird that flies through unmatched blocks does not pass.

Browser-only or vanilla-game-only reduced mode remains useful for illustration but cannot claim articulated bird flight, grasp physics or native dynamics. Functional mode must expose its backend dependency and a clear unavailable state when that dependency is absent.

## Shared bird force model

Use the same versioned aerodynamic semantics in all five native physics backends and the physics-service frontends: local wing/tail section velocities including angular motion, relative airflow, lift/drag/moments, bounded coefficients and stall/drag behaviour. Apply forces and moments at their actual body-space application points through tested transforms. Do not count native fluid forces and custom forces twice. MuJoCo fluid drag alone and an Isaac USD articulation alone do not produce validated powered crow flight.

A shared Rust numerical module with batched FFI or an existing suitable adapter boundary is preferred to divergent formulas in five languages. Record that choice in an ADR/plan and preserve deterministic neural semantics independently from floating-point engine dynamics.
