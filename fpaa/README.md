# FPAA AARNN Pack

This directory contains the AARNN kernels from this repo that are realistic targets for FPAA implementation.

Two artifact sets are provided:

- `xcos/`: Scilab/Xcos-oriented reference simulations for each FPAA-realizable kernel. Each script emits `struct("time", ..., "values", ...)` workspace variables so the signals can be wired into Xcos `From Workspace` and `To Workspace` blocks.
- `okika/`: Okika Pi.Ka deployment manifests and Raspberry Pi programming helpers for the same kernels. These are the files you can act on here without fabricating opaque vendor project binaries.

Supported kernel groups in this pack:

- Synaptic front-end filtering
- Short-term plasticity (STP)
- Adaptive threshold + slow homeostasis
- Active dendritic nonlinearities
- Gap junction + volume-transmission field coupling
- Morphology-aware transmission (attenuation, coarse delay, myelination, fatigue)
- Triplet/metaplastic scaling and Dale-style sign constraints as a hybrid analog + host-supervised flow

Left in software on purpose:

- Growth, pruning, migration, and topology rewiring
- Sleep/dream replay and world-model loops
- Perceptual prediction/error control loops
- Full 3D morphology construction and routing
- Distributed orchestration and experiment control

Index:

| Kernel | Xcos / Scilab | Okika manifest | Pi.Ka wrapper |
|---|---|---|---|
| Synaptic filter | `xcos/01_synaptic_filter.sce` | `okika/01_synaptic_filter.okika.json` | `okika/program_01_synaptic_filter.py` |
| STP | `xcos/02_short_term_plasticity.sce` | `okika/02_short_term_plasticity.okika.json` | `okika/program_02_short_term_plasticity.py` |
| Adaptive threshold + homeostasis | `xcos/03_adaptive_threshold_homeostasis.sce` | `okika/03_adaptive_threshold_homeostasis.okika.json` | `okika/program_03_adaptive_threshold_homeostasis.py` |
| Active dendrite | `xcos/04_active_dendrite.sce` | `okika/04_active_dendrite.okika.json` | `okika/program_04_active_dendrite.py` |
| Gap junction + field | `xcos/05_gap_junction_field.sce` | `okika/05_gap_junction_field.okika.json` | `okika/program_05_gap_junction_field.py` |
| Morphology transmission | `xcos/06_morphology_transmission.sce` | `okika/06_morphology_transmission.okika.json` | `okika/program_06_morphology_transmission.py` |
| Triplet/scaling/Dale hybrid | `xcos/07_triplet_scaling_dale_hybrid.sce` | `okika/07_triplet_scaling_dale_hybrid.okika.json` | `okika/program_07_triplet_scaling_dale_hybrid.py` |

Machine-readable summary: `algorithms.json`.

## Rust runtime integration

The main Rust application now understands this FPAA pack at startup.

Detection:

- Pi.HAT mode probes the configured SPI device, defaulting to `/dev/spidev0.0`, and also checks for GPIO availability
- USB mode probes `/dev/ttyUSB*`, `/dev/ttyACM*`, and `/dev/serial/by-id`
- transport preference and startup policy live in `NetworkConfig.fpaa`

Verification:

- each kernel route is checked against its `*.okika.json` manifest
- the runtime expects the named `.ahf` export to exist beside the manifest
- a successful hardware programming step is expected to leave `fpaa/runtime_state.json`
- repeated programming runs accumulate per-kernel records in `loaded_kernels` (merged by `kernel_id`)
- the runtime compares the persisted `.ahf` fingerprint against the local `.ahf` and also checks the recorded transport type

Supported startup behavior:

- `auto`: probe hardware and fall back to software if not ready
- `disabled`: skip hardware probing and keep all kernels in software
- `required`: fail startup if the requested FPAA routes are not actually verified

CLI examples:

```bash
# Show full startup status and exit
cargo run --bin aarnn_rust -- --fpaa-status-only

# Request FPAA for synaptic filtering and STP
cargo run --bin aarnn_rust -- \
  --fpaa-route synaptic_filter=fpaa \
  --fpaa-route stp=fpaa

# Force USB probing with a product-name hint
cargo run --bin aarnn_rust -- \
  --fpaa-transport usb \
  --fpaa-usb-hint okika \
  --fpaa-print-status
```

UI:

- in the native egui app, open `AARNN -> Biological Realism -> FPAA Offload`
- each kernel shows both the requested route and the effective route
- effective route becomes `FPAA` only when transport readiness, manifest validation, persisted programming state, and sample tests all pass

Current limit:

- this pack supports startup detection, verification, and route selection
- it does not yet provide a live analog execution path from `Runner` into the FPAA, so the Rust algorithms remain the source of truth and the runtime falls back to software whenever hardware use cannot be proven safe
