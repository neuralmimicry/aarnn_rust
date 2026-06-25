# `aer_fabric_bridge`

`aer_fabric_bridge` is a synapse-addressed AER fabric node for distributed FPAA experiments on Raspberry Pi 4 + Okika Pi.Ka hardware.

## Core design rule

The synapse is the event.

- producers share a `synapse_id` (axon boutons, sensory inputs)
- consumers share that same `synapse_id` (dendrite boutons, motor outputs, host mirrors)
- local endpoints are routed locally
- UDP is used only for remote endpoints or explicit host mirroring

## What this bridge does

- receives and sends compact binary AER packets over UDP (`45881`)
- advertises/discovers peers over multicast control (`239.192.44.44:45880`)
- allocates stable node slots by persistent UUID registry (`node_slots.toml`)
- routes local same-FPAA/same-PIKA/same-bridge events without UDP
- forwards remote synapse events over UDP unicast
- supports mock GPIO/SPI backends so it runs on Linux without Pi.Ka hardware
- supports Linux GPIO/SPI backends (`libgpiod`/`spidev`) behind feature flags
- detects FPAA availability at startup and falls back to software kernels when unavailable
- logs service lifecycle, network discovery events, and periodic AER utilisation snapshots via `tracing`
- serves bridge telemetry snapshots on the control plane (`TelemetryRequest` / `TelemetrySnapshot`) so orchestrators can confirm per-node FPAA hardware/config/fallback status

## Discovery and routing model

1. nodes emit periodic `HELLO` control messages over multicast
2. peers are added to a peer table and assigned stable slots
3. route table maps peer slot -> event socket and optional synapse ranges
4. router checks local synapse table first
5. local consumers are stimulated directly
6. remote consumers produce outbound UDP events with decremented TTL

## Run with mock hardware

```bash
cargo run -- run --config ./config/node.toml
```

## Run with Linux GPIO/SPI backends

```bash
cargo run --features linux-gpio,linux-spi -- run --config ./config/node.toml
```

Set in `node.toml`:

- `hardware.gpio_backend = "linux"`
- `hardware.spi_backend = "linux"`
- `hardware.gpio_chip`, `hardware.spi_device`, `hardware.spi_speed_hz`, `hardware.spi_mode`

If FPAA transport probing fails, local stimuli are fulfilled by software kernels on the bridge.

## Validate config

```bash
cargo run -- validate-config --config ./config/node.toml
```

## Send a test event

```bash
cargo run -- send-test-event \
  --synapse 0x5001000200001234 \
  --target 127.0.0.1:45881
```

## Configure synapses

Edit [`config/synapses.toml`](config/synapses.toml). The router uses `synapse_id` as the routable event address and fans out to endpoint consumers by route type.

For local consumers you may optionally set `kernel = "..."` (for example `synaptic_filter`, `short_term_plasticity`, `active_dendrite`, `morphology_transmission`) to select the software fallback kernel used when FPAA is unavailable.

## Hardware integration points

- `src/hardware/gpio.rs`: GPIO abstraction
- `src/hardware/spi.rs`: SPI abstraction
- `src/hardware/pika.rs`: Pi.Ka host facade (`detect_fpaa`, `configure_fpaas`, `stimulate_endpoint`)
- `src/hardware/software_kernel.rs`: software fallback implementations aligned to AARNN kernel families
- Linux hardware backends are feature-gated:
  - `linux-gpio` (line pulse output + edge capture)
  - `linux-spi` (transport probe + transfer/write)
