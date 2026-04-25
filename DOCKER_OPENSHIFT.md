# OpenShift & Multi-Architecture Deployment Guide

This project supports multi-architecture builds and distributed deployment on RedHat OpenShift, targeting a variety of hardware (Intel/ARM/AMD CPUs, NVIDIA/AMD GPUs, FPGAs, and TPUs).

## 1. Multi-Architecture Container Build

The project uses a `Containerfile` based on Ubuntu 24.04.

> **Note**: Container builds no longer compile AARNN from source inside the image. Instead, `scripts/build_container.sh` stages a workload-specific `.deb` package for the native architecture and the `Containerfile` installs that package into the runtime image. This keeps the image build path aligned with the binary release artifacts and avoids repeated in-image Rust builds.

### Build with Podman (Recommended for OpenShift)
```bash
./build_container.sh ghcr.io/neuralmimicry/aarnn_rust engine
```
By default this builds and pushes the native-architecture workload images from the same source tree, stages the matching workload `.deb` package for the host architecture, and assembles a manifest tag per workload:

- `engine-standalone`
- `engine-orchestrator`
- `engine-node`
- `engine-web-ui`
- `engine-desktop-ui`

Limit the build to a subset of workloads by passing a CSV list as the fourth argument:

```bash
./build_container.sh ghcr.io/neuralmimicry/aarnn_rust engine true orchestrator,node,web-ui
```

Skip the automatic push by passing `false` as the third argument.

### Build with Docker Buildx
First stage the package you want the `Containerfile` to install:

```bash
./scripts/prepare_container_package.sh --workload orchestrator
```

Then build the image:

```bash
docker buildx build --platform linux/amd64 \
  -t ghcr.io/neuralmimicry/aarnn_rust:engine-orchestrator \
  --build-arg CONTAINER_WORKLOAD=orchestrator \
  --build-arg CARGO_FEATURES=orchestrator_workload \
  -f Containerfile \
  --push .
```

## 2. Deployment Architecture

The application now builds role-specific images from the same source tree instead of relying on one `--all-features` image for every function.

### Workload / feature matrix

| Workload image | Cargo feature bundle | Effective feature set |
| --- | --- | --- |
| `standalone` | `standalone_workload` | `engine_runtime` |
| `orchestrator` | `orchestrator_workload` | `engine_runtime` |
| `node` | `node_workload` | `engine_runtime` |
| `web-ui` | `web_ui_workload` | `engine_runtime` |
| `desktop-ui` | `desktop_ui_workload` | `engine_runtime + ui + image_input + robot_io` |

`engine_runtime` expands to `parallel + sysinfo + opencl + obs + shmem + growth3d + morpho`.

### Runtime roles

1.  **Standalone CLI**: A single pod running a continuous simulation.
2.  **Standalone with Rust UI**: Same pod but launched with the native UI (X11/VNC required).
3.  **Distributed - Orchestrator**: A central management node that coordinates work and provides a gRPC API.
4.  **Distributed - Worker Node**: Scalable compute nodes that join the orchestrator to perform simulation steps.
5.  **Web UI Server**: A lightweight HTTP server that connects to the orchestrator/nodes and serves the browser UI.

## 3. Repository Structure (SDLC)

The `deploy/` directory is structured using Kustomize to support various environments:

- `deploy/base/`: Common manifests (Deployments, Services, Routes).
- `deploy/overlays/developer/`: Minimal resource requests for local/dev testing.
- `deploy/overlays/alpha/`, `beta/`, `test/`: Progression environments.
- `deploy/overlays/pre-prod/`: Mirrors production configuration.
- `deploy/overlays/prod/`: High-availability, resource-optimized, and hardware-aware.
- `deploy/overlays/release/`: Final stable release artifacts.

### Deploying to a specific environment:
```bash
oc apply -k deploy/overlays/prod
```

## 4. Hardware Acceleration & Mixed CPU Support

### Multi-Arch (Intel/AMD vs ARM)
The container image is multi-arch. OpenShift will automatically pull the correct image for the node's architecture. 
Specific deployments can be pinned to architectures using NodeAffinity (see `deploy/base/patches/affinity-arm64.yaml`).

### Specialized Hardware (GPU/FPGA/TPU)
To utilize specialized hardware, the `node` deployment should be patched with resource limits. 
The application automatically detects these resources via the `Join` gRPC call (defined in `distributed.proto`).

Example resource request in a patch:
```yaml
resources:
  limits:
    nvidia.com/gpu: 1
    # or amd.com/gpu: 1
    # or xilinx.com/fpga: 1
    # or google.com/tpu: 1
```

## 5. Canary Deployments

Canary deployments are handled via OpenShift Routes. The `deploy/overlays/canary` demonstrates how to split traffic between a stable `orchestrator` and a `canary` version.

```bash
oc apply -k deploy/overlays/canary
```
This configuration sends 80% of traffic to the stable service and 20% to the canary service.

## 6. Troubleshooting & Common Issues

### Python Dependency Backtracking & Timeouts
The `requirements.txt` is optimized for multi-architecture builds:
- `PyQt6` is pinned to `6.7.1` and installed with `--only-binary` to ensure `aarch64` wheels are used. This prevents slow, failing source builds that require `qmake`.
- `onnx`, `onnxruntime`, and `tflite-support` are constrained and installed with `--only-binary` to prevent backtracking to old versions that attempt to build from source (which requires `cmake` and other tools not present in the runtime image).
- `numpy` is pinned to `<2.0.0` and installed in a dedicated layer to improve build resilience and caching.
- `tensorflow` usage is branched: `tensorflow-cpu` is used on `x86_64` to reduce image size, while full `tensorflow` is used on `aarch64` because `tensorflow-cpu` wheels are not available for Python 3.9 on that platform.
- The `Containerfile` uses multiple `RUN` steps for `pip` and very high timeouts (`--default-timeout=3600`) and increased retries to handle large packages like TensorFlow over potentially unstable connections.

### Build Script Robustness
The `build_container.sh` script is configured with `set -e` to ensure the multi-architecture manifest is not pushed if any of the individual architecture builds fail. This prevents "half-baked" releases.

### "exec format error" during build
If building for `arm64` on an `x86_64` host fails with an execution error, you likely need to register QEMU static binaries in your kernel's `binfmt_misc`.
Run the following command:
```bash
podman run --rm --privileged docker.io/multiarch/qemu-user-static --reset -p yes
```

### Docker Hub Authentication (Unauthorized)
Docker Hub now requires a Personal Access Token (PAT) for most operations when logged in. If you see an "unauthorized" or "invalid username/password" error:
1. Log in to [hub.docker.com](https://hub.docker.com).
2. Go to **Account Settings** -> **Security** -> **New Access Token**.
3. Use this token instead of your password when running:
   ```bash
   podman login docker.io
   ```

### Registry Resolution (localhost vs docker.io)
By default, Podman might try to resolve short image names against `localhost`. Always use the fully qualified image name (e.g., `docker.io/multiarch/...`) to avoid "connection refused" errors to `localhost:443`.

## 7. Communication & Discovery

- **Nodes to Orchestrator**: Nodes discover the orchestrator via the internal K8s Service name `http://orchestrator:50051`.
- **Web UI to Orchestrator**: The web UI server connects to `http://orchestrator:50051` for gRPC.
- **Browser to Web UI**: Expose `web-ui` on port 8080 via a Route/Ingress.
- **Inter-Node**: Nodes communicate via gRPC streams as assigned by the orchestrator.
- **Local Multi-node**: For local testing, multiple instances can be run in the same pod using different ports or in the same namespace using unique names.

## 8. Interactive Graphical User Interface (UI)

The project includes a native Rust UI (`egui`) and a Python animation UI (`matplotlib`/`PyQt6`). Both are supported in the container but require specific build and deployment steps.

### Building with UI Support

The native UI and IPC/image providers now live in the dedicated `desktop-ui` workload image. Build only that workload when you need an X11-capable container:

```bash
./build_container.sh ghcr.io/neuralmimicry/aarnn_rust engine true desktop-ui
```

### Deploying the UI to OpenShift

Since the UI is a native graphical application, it requires an X server. The most portable way to access this in OpenShift is via a **VNC sidecar** and **noVNC** (web-based VNC client).

An overlay is provided at `deploy/overlays/ui` that:
1.  Adds an `Xvfb` (X virtual framebuffer) and `noVNC` sidecar.
2.  Shares an X11 socket between the application and the sidecar.
3.  Exposes the noVNC interface via an OpenShift Route.

**To deploy:**
```bash
oc apply -k deploy/overlays/ui
```

> **Security Note**: The UI overlay uses a VNC sidecar that may require specific Security Context Constraints (SCC) in some OpenShift environments depending on the cluster's default policy. Ensure the service account has permission to run the sidecar container.

### Accessing the UI

1.  Find the Route URL:
    ```bash
    oc get route standalone-ui
    ```
2.  Open the URL in your web browser.
3.  You will see the desktop session where the `aarnn_rust` UI is running.

> **Tip**: If you prefer to use a local X server (like XQuartz or VcXsrv), you can modify the deployment to point the `DISPLAY` environment variable to your local machine's IP, though this often requires specific network and firewall configurations.

## 9. Web UI Server

The web UI is a separate binary (`web_ui`) and now ships as the dedicated `engine-web-ui` image. It serves an HTTP frontend that connects to the orchestrator and nodes via gRPC.

### Deploying the Web UI to OpenShift

The base manifests include `deploy/base/web-ui.yaml` which provides a Deployment, Service, and Route:

```bash
oc apply -k deploy/base
```

If you prefer to deploy only the web UI, apply the resource directly:

```bash
oc apply -f deploy/base/web-ui.yaml
```

### Web UI Networking

- Web UI server listens on `0.0.0.0:8080`.
- Web UI server connects to the orchestrator at `http://orchestrator:50051`.
- Nodes connect to the orchestrator service at `http://orchestrator:50051`.
- Expose the web UI with a Route/Ingress to allow browsers to access it.

## 10. Running the Images in Different Modes

Each role has a dedicated image tag. The container entrypoint supplies a sensible default command for that role, and you can still override args explicitly when needed.

```bash
# CLI-only (single host)
podman run --rm ghcr.io/neuralmimicry/aarnn_rust:engine-standalone

# Rust UI (requires X11 or VNC sidecar)
podman run --rm -e DISPLAY=$DISPLAY -v /tmp/.X11-unix:/tmp/.X11-unix ghcr.io/neuralmimicry/aarnn_rust:engine-desktop-ui

# Orchestrator
podman run --rm -p 50051:50051 -p 50050:50050/udp ghcr.io/neuralmimicry/aarnn_rust:engine-orchestrator

# Node
podman run --rm -p 50052:50052 ghcr.io/neuralmimicry/aarnn_rust:engine-node \
  --node --orchestrator-addr http://<orchestrator-host>:50051 --grpc-addr 0.0.0.0:50052

# Web UI server
podman run --rm -p 8080:8080 ghcr.io/neuralmimicry/aarnn_rust:engine-web-ui \
  --listen 0.0.0.0:8080 --orchestrator http://<orchestrator-host>:50051
```

Local Podman test scripts matching these workloads are available at:

- `run_container_standalone.sh`
- `run_container_orchestrator.sh`
- `run_container_node.sh`
- `run_container_web_ui.sh`
- `run_container_desktop_ui.sh`
- `run_container_cluster.sh`
