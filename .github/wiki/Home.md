# AARNN Rust — Wiki Home

**aarnn_rust** is NeuralMimicry's full neuromorphic autonomous AI platform, written in Rust. It combines a GPU-accelerated spiking neural network runtime with a Kubernetes operator, continuous training pipelines, embodied simulation environments (Webots, NAO), FPAA hardware integration, and a web-based control surface — all deployable as a single self-contained service or as a distributed Kubernetes workload.

> ☕ **[Support NeuralMimicry on Crowdfunder](https://www.crowdfunder.co.uk/p/qr/aWggxwPW?utm_campaign=sharemodal&utm_medium=referral&utm_source=shortlink)** — independent open-source initiative.

---

## Contents

- [What it does](#what-it-does)
- [Architecture](#architecture)
- [Source module map](#source-module-map)
- [GPU compute backends](#gpu-compute-backends)
- [FPAA hardware integration](#fpaa-hardware-integration)
- [Embodied runtimes](#embodied-runtimes)
- [Kubernetes operator](#kubernetes-operator)
- [Quick start](#quick-start)
- [Configuration](#configuration)
- [Training and experiments](#training-and-experiments)
- [Documentation](#documentation)
- [Related repositories](#related-repositories)
- [Get involved](#get-involved)

---

## What it does

| Capability | Description |
|---|---|
| **Spiking neural network runtime** | Biologically-grounded LIF, Izhikevich, and AARNN neuron models with Hebbian, STDP, and triplet plasticity rules |
| **GPU acceleration** | OpenCL (primary) and CUDA fallback; separate kernels for synaptic filtering, STP, plasticity, morphological energy, and LIF stepping |
| **Morphological adaptation** | Growth, dendritic integration, and structural plasticity — networks reconfigure their topology at runtime without retraining |
| **AER encoding** | Address-Event Representation: spikes encoded as AER1 binary payloads for transport to Gail, FPAA hardware, or remote runtimes |
| **Webots embodied simulation** | Digital-twin bodies (humanoid NAO, hexapod, custom) driven by the spiking network in real-time |
| **FPAA hardware integration** | Seven algorithm families (synaptic filter, STP, adaptive threshold, active dendrite, gap junction, morphology, triplet scaling) compiled for Okika FPAA and Scilab/Xcos |
| **Kubernetes operator** | CRDs for `Model`, `Experiment`, `Dataset`; Argo Rollouts canary delivery; Istio traffic splitting; shadow deployment and A/B routing |
| **Web UI** | Operator control surface at `src/bin/web_ui.rs`; live network visualisation and runtime control |
| **Distributed runtime** | OpenMPI-backed multi-node execution for large-scale network simulations |
| **Training pipeline** | QLoRA/LoRA integration, dataset curation from traces, model registry via Gail |

---

## Architecture

```
┌──────────────────────────────────────────────────────────┐
│                      Web UI / REST API                   │
│              (src/bin/web_ui.rs · src/runtime_api.rs)    │
└───────────────────────────┬──────────────────────────────┘
                            │
┌───────────────────────────▼──────────────────────────────┐
│                     Runtime engine                       │
│   src/engine.rs · src/runtime.rs · src/network.rs        │
│   src/morphology.rs · src/sim.rs                         │
│                                                          │
│  ┌──────────────┐  ┌────────────┐  ┌──────────────────┐  │
│  │  Neuron core │  │ Plasticity │  │  Spike I/O       │  │
│  │ src/aarnn/   │  │ src/aarnn/ │  │ src/spike_io/    │  │
│  │ dynamics.rs  │  │plasticity.rs│  │ encoding.rs      │  │
│  │transmission.rs│  └────────────┘  │ transport.rs     │  │
│  └──────┬───────┘                   └────────┬─────────┘  │
│         │                                    │            │
│  ┌──────▼───────────────────┐       ┌────────▼─────────┐  │
│  │   GPU compute            │       │   AER bridge     │  │
│  │ src/cl_compute.rs (OCL)  │       │ src/aer.rs       │  │
│  │ src/gpu_api.rs   (CUDA)  │       │ src/aer_can.rs   │  │
│  └──────────────────────────┘       └──────────────────┘  │
└──────────────────────────────────────────────────────────┘
           │                                  │
    ┌──────▼──────┐                   ┌───────▼──────┐
    │ FPAA target │                   │  Gail bridge │
    │  src/fpaa.rs│                   │  src/bridge.rs│
    │ fpaa/okika/ │                   └──────────────┘
    └─────────────┘
```

---

## Source module map

| Module | File | Purpose |
|---|---|---|
| Neuron dynamics | `src/aarnn/dynamics.rs` | LIF, Izhikevich, AARNN neuron stepping |
| Synaptic transmission | `src/aarnn/transmission.rs` | AMPA/NMDA/GABA decay, synaptic gap modelling |
| Plasticity | `src/aarnn/plasticity.rs` | Hebbian, STDP, triplet rules |
| Engine | `src/engine.rs` | Main simulation loop, per-step orchestration |
| Network | `src/network.rs` | Topology management, connectivity, clustering |
| Morphology | `src/morphology.rs` | Structural growth, dendritic integration, energy updates |
| Runtime | `src/runtime.rs` | Service lifecycle, startup/shutdown |
| Runtime API | `src/runtime_api.rs` | HTTP REST endpoints for operator control |
| GPU (OpenCL) | `src/cl_compute.rs` | OpenCL kernels: LIF, STP, plasticity, morphological energy |
| GPU (CUDA) | `src/gpu_api.rs` | CUDA fallback; shared GPU dispatch interface |
| AER encoding | `src/aer.rs` · `src/aer_can.rs` | AER1 varint-encoded spike events; CAN bus variant |
| Spike I/O | `src/spike_io/` | Encoding profiles, transport layer, UDS/TCP |
| FPAA | `src/fpaa.rs` | FPAA algorithm compilation and execution |
| Bridge | `src/bridge.rs` | Gail AI middleware bridge (AER mirroring) |
| Stimuli | `src/stimuli.rs` | Sensory input injection (audio, vision, proprioception) |
| Deployment | `src/deployment.rs` | Kubernetes CRD controllers |
| Distributed | `src/distributed.rs` | OpenMPI multi-node runtime |
| Simulation | `src/sim.rs` | Headless simulation runner |
| Web UI | `src/bin/web_ui.rs` | Operator dashboard binary |
| Providers | `src/providers.rs` | Compute backend registry (OpenCL, CUDA, CPU) |
| Service access | `src/service_access.rs` | NeuralMimicry `service_access` auth contract |
| nmchain | `src/nmchain.rs` | Token ledger event emission |
| Affinity | `src/affinity.rs` | NUMA/core affinity for parallel execution |
| Shared FS | `src/shared_fs.rs` | NFS-backed shared storage for Kubernetes deployments |
| Monitor | `src/monitor.rs` | Health, telemetry, and metrics export |

---

## GPU compute backends

The platform supports two GPU backends selected at runtime:

### OpenCL (primary — `src/cl_compute.rs`)

Seven GPU kernels compiled at startup:
1. **LIF stepping** — integrate-and-fire neuron update per component
2. **Izhikevich** — two-variable spiking model
3. **Synaptic accumulation** — sparse/dense post-synaptic current summation with optional STP
4. **Synaptic filtering** — AMPA/NMDA/GABA exponential decay
5. **Plasticity update** — Hebbian rule across active synapses
6. **Morphological energy** — growth energy update for dendritic and axonal components
7. **Dendrite integration** — postsynaptic integration across branching dendrite trees

Kernels operate on **columnar SoA buffers** (`Vec<f32>` per attribute) for cache-friendly GPU access.

### CUDA fallback (`src/gpu_api.rs`)

Activated when OpenCL is unavailable. Uses `cudarc` with CUDA 12. Hardware detection via `/dev/nvidia0` probe and `nvidia-smi` enumeration.

### CPU path

Rayon-based chunked parallel iteration on row-structured data. Used when no GPU is present or for debugging.

---

## FPAA hardware integration

The `fpaa/` directory contains seven biologically-grounded algorithm families compiled for two FPAA targets:

| Algorithm | Okika source | Scilab/Xcos source |
|---|---|---|
| Synaptic filter | `fpaa/okika/01_synaptic_filter.okika.json` | `fpaa/xcos/01_synaptic_filter.sce` |
| Short-term plasticity | `fpaa/okika/02_short_term_plasticity.okika.json` | `fpaa/xcos/02_short_term_plasticity.sce` |
| Adaptive threshold + homeostasis | `fpaa/okika/03_adaptive_threshold_homeostasis.okika.json` | `fpaa/xcos/03_...sce` |
| Active dendrite | `fpaa/okika/04_active_dendrite.okika.json` | `fpaa/xcos/04_active_dendrite.sce` |
| Gap junction field | `fpaa/okika/05_gap_junction_field.okika.json` | `fpaa/xcos/05_gap_junction_field.sce` |
| Morphology transmission | `fpaa/okika/06_morphology_transmission.okika.json` | `fpaa/xcos/06_...sce` |
| Triplet scaling + Dale's law hybrid | `fpaa/okika/07_triplet_scaling_dale_hybrid.okika.json` | `fpaa/xcos/07_...sce` |

FPAA partitioning strategy: [`docs/aarnn_fpaa_partitioning.md`](https://github.com/neuralmimicry/aarnn_rust/blob/main/docs/aarnn_fpaa_partitioning.md)

---

## Embodied runtimes

The `examples/` directory contains embodied robot controllers that drive real or simulated bodies using the AARNN network:

| Example | Target | Interface |
|---|---|---|
| `nao_runner.rs` | NAO humanoid robot | Soft-real-time Rust controller |
| `nao_nn_controller.cpp` | NAO (C++ variant) | NAOqi SDK / ALProxies |
| `nao_nn_controller_uds.cpp` | NAO via UDS | Unix domain socket bridge |
| `webots_controller.cpp` | Webots simulator | Webots C++ API |
| `robot_spike_probe.rs` | Any embodied target | Live spike probe and logging |
| `uds_latency_client/server.rs` | Benchmarking | UDS round-trip latency measurement |

NAO mapping reference: [`examples/nao_mapping.rs`](https://github.com/neuralmimicry/aarnn_rust/blob/main/examples/nao_mapping.rs)

---

## Kubernetes operator

The deployment module (`src/deployment.rs`) provides Kubernetes Custom Resource Definitions and controllers:

- `Model` CRD — declares a trained AARNN network checkpoint and its serving configuration
- `Experiment` CRD — defines A/B tests and multi-armed bandit routing policies
- `Dataset` CRD — references training datasets stored on shared NFS

Delivery strategy (via Argo Rollouts):
```
shadow deployment → canary (5% → 25% → 50%) → blue/green promotion
```

Istio traffic policies route spike-event traffic based on header-based weighted matching.

Build and push the container image:

```bash
bash build_container.sh ghcr.io/neuralmimicry/aarnn_rust main
```

The script builds architecture-specific Debian packages (`.container-cache/`) and feeds them into the container image rather than compiling Rust inside Docker.

---

## Quick start

```bash
# Build (debug)
cargo build

# Build (release — recommended for performance)
cargo build --release

# Run the main runtime with default config
cargo run --release -- --config config.json

# Run the web UI
cargo run --release --bin web_ui -- --config config.json

# Run headless simulation
cargo run --release -- --config config.json --headless --steps 1000
```

The Ansible playbook provisions the host end-to-end:

```bash
cd ansible/
ansible-playbook -i inventory.ini playbooks/site.yml
```

---

## Configuration

`config.json` controls network topology, compute backend selection, and runtime parameters:

```json
{
  "network_id": "neuralmimicry-shared-snn",
  "neuron_model": "aarnn",
  "learning_rule": "aarnn",
  "sensory_neurons": 32,
  "hidden_layers": 2,
  "hidden_neurons_per_layer": 128,
  "output_neurons": 16,
  "layer_depth": 6,
  "growth_enabled": true,
  "morphology_enabled": true,
  "clumping_design": "HumanBrain",
  "compute_backend": "opencl",
  "fpaa_enabled": false,
  "gail_bridge_enabled": false
}
```

Key environment variables:

| Variable | Purpose |
|---|---|
| `AARNN_COMPUTE_BACKEND` | `opencl`, `cuda`, or `cpu` |
| `AARNN_GAIL_BRIDGE_URL` | Gail endpoint for AER mirroring |
| `AARNN_NMCHAIN_URL` | nmchain endpoint for token events |
| `AARNN_STATE` | Snapshot file path (default `aarnn_state.bin`) |
| `RUST_LOG` | Log level (`info`, `debug`, `trace`) |

---

## Training and experiments

| Directory | Contents |
|---|---|
| `experiments/ab/` | A/B testing framework for comparing network variants |
| `experiments/bandits/` | Multi-armed bandit routing for online network selection |
| `experiments/rl/` | Reinforcement learning router for adaptive policy selection |

Training data flows from the Gail LLM interaction ledger → AARNN trace dataset → QLoRA fine-tuning → model snapshot → Kubernetes `Model` CRD update.

---

## Documentation

| Document | Path |
|---|---|
| Architecture | [`docs/architecture.md`](https://github.com/neuralmimicry/aarnn_rust/blob/main/docs/architecture.md) |
| Operations | [`docs/operations.md`](https://github.com/neuralmimicry/aarnn_rust/blob/main/docs/operations.md) |
| Operator guide | [`docs/operator.md`](https://github.com/neuralmimicry/aarnn_rust/blob/main/docs/operator.md) |
| FPAA partitioning | [`docs/aarnn_fpaa_partitioning.md`](https://github.com/neuralmimicry/aarnn_rust/blob/main/docs/aarnn_fpaa_partitioning.md) |
| Growth workflow | [`docs/growth_workflow_alignment.md`](https://github.com/neuralmimicry/aarnn_rust/blob/main/docs/growth_workflow_alignment.md) |
| Security | [`SECURITY.md`](https://github.com/neuralmimicry/aarnn_rust/blob/main/SECURITY.md) |
| Compliance | [`COMPLIANCE.md`](https://github.com/neuralmimicry/aarnn_rust/blob/main/COMPLIANCE.md) |

---

## Related repositories

| Repository | Relationship |
|---|---|
| [aarnn](https://github.com/neuralmimicry/aarnn) | Original C++ AARNN platform with PostgreSQL persistence and VTK visualisation |
| [gail](https://github.com/neuralmimicry/gail) | AI middleware: mirrors LLM I/O as AER spike trains; trains models on interaction data |
| [aarnn-nsys](https://github.com/neuralmimicry/aarnn-nsys) | Ultra-low-latency pub/sub bus used for inter-process spike event transport |
| [feel-bridge](https://github.com/neuralmimicry/feel-bridge) | FPAA research underpinning the hardware algorithms in `fpaa/` |
| [nmc](https://github.com/neuralmimicry/nmc) | Kubernetes operator CLI that manages deployed AARNN clusters |
| [tracey](https://github.com/neuralmimicry/tracey) | Security and resilience runtime; monitors AARNN fleet health |

---

## Get involved

- 🐛 [Report a bug or request a feature](https://github.com/neuralmimicry/aarnn_rust/issues)
- 💬 [Join the discussion](https://github.com/neuralmimicry/aarnn_rust/discussions)
- 📧 Direct support from Paul Isaac's (Founder & CTO): [info@neuralmimicry.ai](mailto:info@neuralmimicry.ai) · **£1,000/day + VAT**
- 🌐 [neuralmimicry.ai/aarnn-neuroscience](https://neuralmimicry.ai/aarnn-neuroscience)
