# AARNN SPICE kernel references

This directory is the circuit-level companion to `fpaa/xcos`, `fpaa/okika`, and
the Rust kernels in `src/aarnn`. It contains portable SPICE macromodels, not
vendor configuration bytes. The models are deliberately equation-level: OTA-like
behavioral current sources drive exposed capacitive state nodes. That makes the
files useful for three jobs:

1. regression against the software/Xcos golden model;
2. sizing a conventional PCB implementation;
3. translating the stages into Anadigm Designer 2 CAMs for later AHF export.

## Files and signal contract

`aarnn_kernels.lib` contains the seven project kernel families:

| Subcircuit | Project kernel | Analog implementation | Exactness boundary |
|---|---|---|---|
| `AARNN_SYNAPTIC_FILTER` | AMPA/NMDA/GABA filter | three OTA-C leaky states, signed split, NMDA bias gate, summer | NMDA gate is a logistic approximation |
| `AARNN_STP` | short-term plasticity | two slow capacitors plus `u*x` multiplier | spike impulse and clamps need calibration |
| `AARNN_ADAPTIVE_HOMEOSTASIS` | adaptive threshold/homeostasis | two integrators and error OTA | target and rail clamps are biases/rails |
| `AARNN_ACTIVE_DENDRITE` | calcium/plateau dendrite | calcium integrator, trigger, plateau integrator, gain path | branch structure is a bias, not topology |
| `AARNN_GAP_PAIR` / `AARNN_VOLUME_FIELD` | gap junction/field | programmable conductance and slow spatial bias | geometry is compiled/routed externally |
| `AARNN_MORPH_TRANSMISSION` | morphology transmission | three-section coarse cable plus programmable gain | exact event delay/jitter remain hybrid |
| `AARNN_TRIPLET` | triplet/scaling/Dale hybrid | three trace integrators and two multiplications | row scaling/sign enforcement remain host-side |

Signals use volts as normalized values (`1 V = 1 AARNN unit`). `VM` is the one
exception: it is a physical voltage expressed in SPICE volts, so `-65 mV` means
the software membrane value `-65`. All state pins are exposed for measurement.
The SPICE files assume ngspice/LTspice-style behavioral sources (`B` elements,
ternary expressions, `min`, `max`, `tanh`, and `exp`). A vendor CAM must replace those expressions with
available OTA, comparator, multiplier, and bias blocks.

## Running the smoke test

From this directory:

```sh
ngspice -b -o tb_aarnn_kernels.log tb_aarnn_kernels.cir
```

The repository does not commit a simulator or vendor `.ahf`. If ngspice is not
installed, the netlist can still be opened in LTspice after changing the output
commands to that simulator's syntax.

## PCB realization

Use a dual-rail analog supply (for example ±5 V) for the first prototype so the
signed synaptic and gap-junction signals do not need a virtual-ground translation.
The following is a practical stage-level BOM, not a claim that these exact parts
are electrically interchangeable with an FPAA:

| Function | PCB primitive | Starting values / notes |
|---|---|---|
| leaky state | LM13700 OTA or precision op-amp integrator | `C=1 uF`; `R=tau/C`: 5 kΩ, 10 kΩ, 100 kΩ, 200 kΩ, 800 kΩ, 2 MΩ, 3.5 MΩ for the project time constants |
| `u*x` and triplet products | AD633 or equivalent four-quadrant multiplier | scale the multiplier's 10 V full-scale input to the 0–1 V normalized contract |
| signed split / plateau trigger | rail-to-rail comparator or precision rectifier | use hysteresis and clamp to the software envelopes |
| programmable gain/weight | digital potentiometer or DAC-controlled OTA bias | 8-bit trim is a sensible first match to the target FPAA precision |
| I/O protection | unity buffer plus series 1 kΩ and clamp diodes | keep external sensor/robot lines away from integrator nodes |

For a single-supply PCB, translate every normalized signal by a 2.5 V virtual
ground and document the translation at the ADC/DAC boundary; do not feed a
negative SPICE signal directly into a 0–5 V FPAA I/O cell.

## AN231E04 / PIKA realization

The public PIKA hardware reference identifies four daisy-chained AN231E04 chips,
20 CABs and four I/O cells per chip, with a local 16 MHz clock and 40 MHz as an
alternative. Map one row of the table above to an Anadigm CAM chain as follows:

1. `OTA-C` or OTA low-pass CAMs implement each exposed state capacitor.
2. `GainInv`/`SumInv`-style CAMs implement signed gain, summation, and bias.
3. Comparator/common-source/common-drain CAMs implement rectification and plateau
   recruitment where the selected chip family provides those primitives.
4. A multiplier/translinear CAM or calibrated two-OTA product implements `u*x`
   and the triplet terms.
5. Use local CAB-to-CAB routes for a neuron tile; reserve I/O cells for AER/host
   boundaries and probe outputs.
6. Export one AHF configuration per chip in chain order, then run the existing
   `fpaa/okika/common/pika_loader.py` flow. The current Rust runtime should only
   mark a route as FPAA after the manifest, AHF fingerprint, transport, and sample
   checks all agree.

SPICE is not directly encodable into an AN231E04. The concrete conversion step is
to recreate the macrocell in Anadigm Designer 2, set CAM parameters from the
`.param` values, wire the contacts, and export AHF. Public tooling documents a
scriptable COM path (`Workspace.Chips.Add(9, ...)`, CAM parameter setters, and
`WriteConfigData(..., 0)`) for that step. The existing Okika manifests remain the
source of truth for the expected AHF name and SPI transport.

## What stays hybrid

The SPICE cells intentionally do not hide limitations already called out by the
Rust partitioning document:

- growth, pruning, migration, topology, and distributed routing remain software;
- exact morphology-dependent integer delay and deterministic jitter are host
  metadata; the three-section transmission cell is only a coarse analog delay;
- synaptic row normalization, Dale-law sign enforcement, and deterministic release
  heterogeneity are supervisory/compile-time operations;
- the existing runtime still uses software as the numerical source of truth until
  a live FPAA data path is implemented.

## Internet references checked

These are the sources used to constrain the design, accessed 2026-08-07:

- [Anadigm AN231E04 product page](https://anadigm.com/an231e04.asp)
- [Anadigm technical overview](https://anadigm.com/tech_overview.asp)
- [Anadigm AN231E04 datasheet](https://anadigm.com/_doc/DS021000-U004.pdf)
- [Anadigm AN231E04 user manual](https://anadigm.com/_doc/UM231004-K002.pdf)
- [Public PIKA hardware reference](https://github.com/bshepp/fpaa-tools/blob/main/docs/PIKA_HARDWARE.md)
- [Public PIKA/AN231E04 tooling notes](https://github.com/bshepp/fpaa-tools/blob/main/README.md)
- [Public Anadigm Designer 2 automation notes](https://github.com/bshepp/fpaa-tools/blob/main/docs/AD2_COM_AUTOMATION.md)

The Anadigm pages currently redirect through their landing page, so the public
PIKA reference is also included as an operational cross-check. Before fabrication
or AHF export, verify the exact CAM names, voltage ranges, capacitor limits, and
I/O electrical characteristics against the licensed Designer/device documentation
for the actual board revision.
