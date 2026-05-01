# Okika Pi.Ka Deployment Pack

This directory contains Pi.Ka-oriented manifests and helpers for the AARNN kernels that map cleanly or semi-cleanly onto FPAA circuitry.

What is here:

- `*.okika.json`: per-kernel deployment manifests, default parameters, suggested analog stage breakdown, and the expected `.ahf` artifact name.
- `program_*.py`: wrapper scripts that call the shared Pi.Ka loader for one manifest.
- `common/pika_loader.py`: a reusable Raspberry Pi SPI loader derived from the public Pi.Ka programming flow.

What is intentionally not here:

- fabricated `.okt`, `.ad2`, or `.ahf` vendor project binaries
- guessed bitstreams

Expected flow:

1. Use the manifest to recreate the kernel in DynAMx Design Lab or Anadigm Designer.
2. Export a single-file primary `.ahf` using the expected file name from the manifest.
3. Copy the `.ahf` next to the manifest.
4. Validate with a dry run:
   `python3 common/pika_loader.py 01_synaptic_filter.okika.json --dry-run`
5. On a Raspberry Pi with the Pi.Ka attached, program the design:
   `python3 program_01_synaptic_filter.py`

The shared loader assumes:

- SPI on bus `0`, device `0`
- GPIO pinout matching the Pi.Ka quick-start wiring
- local 16 MHz oscillator selected by default
- primary-configuration style transfer with CE0 held low across the byte stream

## Runtime-state handoff to Rust

After a successful Pi.Ka programming step, `common/pika_loader.py` writes:

- `../runtime_state.json`

That file is consumed by the Rust startup probe so the runtime can decide whether
the local FPAA image matches the requested AARNN kernel route.

The probe checks:

- detected transport readiness
- manifest presence and parseability
- expected `.ahf` presence and basic primary-file validity
- `.ahf` fingerprint match against `runtime_state.json`
- persisted transport consistency with the currently detected hardware

Useful commands from the repository root:

```bash
# Show detection / verification status only
cargo run --bin aarnn_rust -- --fpaa-status-only

# Require Pi.HAT hardware and fail if the route is not verified
cargo run --bin aarnn_rust -- \
  --fpaa-mode required \
  --fpaa-transport pihat \
  --fpaa-route synaptic_filter=fpaa
```

USB note:

- the Rust runtime can probe USB-style endpoints during startup
- this directory currently provides the Pi.Ka GPIO/SPI loader and runtime-state writer
- if a board is programmed through a different USB-specific flow, an equivalent `fpaa/runtime_state.json` record is still needed or the Rust runtime will keep the effective route in software
