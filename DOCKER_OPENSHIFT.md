# OpenShift & Multi-Architecture Deployment Guide

This project supports multi-architecture builds and distributed deployment on RedHat OpenShift, targeting a variety of hardware (Intel/ARM/AMD CPUs, NVIDIA/AMD GPUs, FPGAs, and TPUs).

## 1. Multi-Architecture Container Build

The project uses a multi-stage `Containerfile` based on CentOS Stream 9.

> **Note**: While RedHat UBI is often preferred for OpenShift, this project uses CentOS Stream 9 as a base because it provides access to necessary dependencies like OpenCL headers, Protobuf compiler, and Netlink development libraries (from the CRB and AppStream repositories). Additionally, OpenCV is built from source during the container build process to ensure availability and consistent versioning across different architectures, as it is not present in the standard CentOS Stream 9 repositories.

### Build with Podman (Recommended for OpenShift)
```bash
./build_container.sh ghcr.io/neuralmimicry/aarnn_rust v1.0.0 true
```
This script creates a manifest for `linux/amd64` and `linux/arm64`, builds both architectures (using QEMU if necessary), and pushes them to GHCR.
The container build produces both binaries: `aarnn_rust` and `web_ui`.

### Build with Docker Buildx
```bash
docker buildx build --platform linux/amd64,linux/arm64 -t ghcr.io/neuralmimicry/aarnn_rust:latest . --push
```

## 2. Deployment Architecture

The application can be deployed in multiple modes from the same image:

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

By default, the container is built **with** the `ui` feature so the image can run CLI, Rust UI, orchestrator, node, and web UI modes. You can override the feature set to reduce image size if needed:

```bash
# Example building with a slimmer feature set (no UI)
./build_container.sh ghcr.io/neuralmimicry/aarnn_rust v1.0.0 true "growth3d,sysinfo,opencl"
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

The web UI is a separate binary (`web_ui`) bundled in the same image. It serves an HTTP frontend that connects to the orchestrator and nodes via gRPC.

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

## 10. Running the Image in Different Modes

All modes use the same container image and differ only by command/args:

```bash
# CLI-only (single host)
podman run --rm ghcr.io/neuralmimicry/aarnn_rust:latest --t 2000

# Rust UI (requires X11 or VNC sidecar)
podman run --rm -e DISPLAY=$DISPLAY -v /tmp/.X11-unix:/tmp/.X11-unix ghcr.io/neuralmimicry/aarnn_rust:latest --ui

# Orchestrator
podman run --rm -p 50051:50051 -p 50050:50050/udp ghcr.io/neuralmimicry/aarnn_rust:latest --orchestrator --grpc-addr 0.0.0.0:50051

# Node
podman run --rm -p 50052:50052 ghcr.io/neuralmimicry/aarnn_rust:latest --node --orchestrator-addr http://<orchestrator-host>:50051 --grpc-addr 0.0.0.0:50052

# Web UI server
podman run --rm -p 8080:8080 ghcr.io/neuralmimicry/aarnn_rust:latest ./web_ui --listen 0.0.0.0:8080 --orchestrator http://<orchestrator-host>:50051
```
