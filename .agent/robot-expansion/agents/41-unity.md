# Implement the Unity target

Extend NmRobotBase/NmAerClient patterns and implement a real profile factory or generated prefabs plus launch/spec registration. NmSimulationManager.robotSpec is currently informational, so a new registry entry alone cannot spawn a robot. Import canonical mechanics and IO with explicit basis/unit transforms. Apply drives in physics timing, capture actual sensor state, and keep bounded background AER queues. Add crow aero, contact-based grasping and task process hooks. Verify both headless/batch and Editor/play-mode evidence on an installed supported Unity runtime, then actual neural loops.

Completion evidence must be recorded in the living ExecPlan before proceeding or reporting this work package complete.
