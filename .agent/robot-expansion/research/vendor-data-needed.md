# Missing model data checklist

This is a prepared checklist for the user or an authorised vendor conversation; no request has been sent.

For each KEENON T8/T9/T10/T11/W3/S100/C30/C40, BOTINKIT OMNI, XBOT S Pro/IC60, COFE+ Robot Café/Smart Bar, and the six UGOT forms without an identified model, obtain:

- Exact product name, SKU, hardware revision, accessory set and software/firmware compatibility.
- Native CAD assembly or STEP plus link-separated meshes; licensed URDF/Xacro, MJCF or USD when available.
- Joint parent/child frames, axes, limits, zero poses, gear ratios, passive/coupled/mimic joints and continuous joint conventions.
- Link mass, centre of mass, inertia tensor/axes, payload attachment frames, suspension/roller/contact data.
- Actuator types, torque/speed/current/temperature limits, friction, damping and command semantics.
- Sensor placement/extrinsics, intrinsics, field of view, rates, noise, latency and SDK/API contracts.
- Dock/service-station/lift-interface simulation requirements, tool/gripper/nozzle frames and device interlocks.
- Explicit rights to convert, modify, redistribute and use meshes, textures, CAD and documentation in the intended project.

Product-specific gaps: resolve both XBOT S Pro size/mass records; distinguish the COFE+ bar and kiosk generations; match Galbot G1 head/arms/end effectors to the official Golf description; choose S100 body depth; select W3 compartment layout and C40 squeegee state; obtain UGOT part/port maps per build.

Do not make original-surrogate work depend on these responses. Keep the exact-replica gate open and label every assumed parameter. A third-party public upload without a clear licence is a lead, not permission to ship a model.
