# Add native MuJoCo for all profiles

Implement the native backend specified in spec/physics-and-engines.md for all 28 profiles. Register --sim mujoco only once backed by real dispatch, readiness, headless/interactive launch, AER endpoints and cleanup. Load/generate MJCF with correct free roots, actuators, sensors, tendons/passive/mimic semantics and force hooks. Resolve qpos/qvel/actuator indices by names, not matching array positions. Keep a single mj_step owner and optional passive viewer. Networking is bounded/asynchronous; reset rebuilds indices and epochs. Validate the bundled Galbot and UGOT native MJCF, then all other profiles. Run neural-loop, camera/contact, replay and multi-world isolation tests. Pin the tested MuJoCo environment separately from Isaac Sim.

Completion evidence must be recorded in the living ExecPlan before proceeding or reporting this work package complete.
