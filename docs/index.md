# Platform Overview

This platform extends the neuromorphic project into a production-oriented autonomous deployment system.

It combines:

- immutable container builds
- operator-managed model lifecycle
- progressive delivery
- experiment-driven promotion
- automated retraining hooks

Additional hardware-offload documentation:

- `../fpaa/README.md`: generated FPAA artifact pack for AARNN kernels
- `../fpaa/okika/README.md`: Pi.Ka deployment flow and runtime-state verification notes
- `../docs/aarnn_fpaa_partitioning.md`: design rationale for which AARNN kernels are realistic FPAA targets

Runtime FPAA support in the Rust application includes:

- startup autodetection for Pi.HAT GPIO/SPI and USB-style endpoints
- startup verification against the Okika manifest, expected `.ahf`, and `fpaa/runtime_state.json`
- host-side sample tests for supported AARNN kernels
- per-kernel requested vs effective routing in the CLI and native UI
- automatic fallback to software whenever hardware is missing, unready, or unverified

Current limitation:

- the runtime exposes detection and routing policy, but the AARNN numerical kernels still execute in software until a concrete FPAA execution path is implemented
