# AARNN → CELLINK BIO X6 exporter starter

This repository is a reference implementation scaffold for converting a frozen
AARNN topology into a calibrated physical-print plan and a machine-dialect
toolpath.

It deliberately does **not** claim that the included G-code templates are
printer-ready. Exact extrusion, pressure, temperature, photocuring and tool
commands must be verified in CELLINK DNA Studio and against the installed BIO
X6 firmware.

## Why there are three representations

1. `AarnnSnapshot`: logical neural semantics, independent of printer geometry.
2. `PhysicalPlan`: material, optical, timing and threshold mapping.
3. `ToolpathBundle`: ordered print operations and a configurable G-code dialect.

This separation is mandatory because a centimetre-scale optical path cannot
directly reproduce millisecond biological delays. The exporter must preserve
relative timing through calibrated fluorescent, photoresponsive, time-bin or
external optoelectronic delay elements.

## Run

```bash
cargo test

cargo run -- validate \
  --input examples/aarnn_snapshot.json \
  --machine config/machine.example.yaml \
  --calibration config/calibration.example.yaml

cargo run -- export \
  --input examples/aarnn_snapshot.json \
  --machine config/machine.example.yaml \
  --calibration config/calibration.example.yaml \
  --output-dir build/demo
```

Open `build/demo/job.gcode` in DNA Studio only after replacing the example
dialect templates with commands verified for the local printer.

## Immediate Codex tasks

1. Implement an adapter from the existing AARNN Rust structs/database rows into
   `AarnnSnapshot`.
2. Replace the orthogonal placeholder router with a constrained 3D router.
3. Add collision, bend-radius, crossing and coaxial-core/cladding planning.
4. Add calibrated splitter, attenuation and power-budget propagation.
5. Add node transfer-curve inversion for threshold mapping.
6. Emit separate preview meshes per material plus a provenance manifest.
7. Add a digital-twin simulation and post-print calibration ingest.
8. Add signed/inhibitory dual-rail routing and detector subtraction.
9. Add deterministic output hashing and complete audit trails.
10. Add a DNA Studio validation checklist and firmware-specific dialect fixture.
