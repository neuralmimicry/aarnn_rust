# Multi-arch Containerfile for AARNN role-specific workloads.
# Target Architectures: linux/amd64, linux/arm64
# Base: Ubuntu 24.04

ARG PYTHON_MIN_VERSION=3.12
ARG PYTHON_FULL_VERSION=3.12.2
FROM ubuntu:24.04
ARG PYTHON_MIN_VERSION=3.12
ARG PYTHON_FULL_VERSION=3.12.2
ARG CONTAINER_WORKLOAD="standalone"
ARG CARGO_FEATURES="standalone_workload"
ENV PYTHON_MIN_VERSION=${PYTHON_MIN_VERSION}
ENV PYTHON_FULL_VERSION=${PYTHON_FULL_VERSION}
ENV AARNN_CONTAINER_WORKLOAD=${CONTAINER_WORKLOAD}
ENV DEBIAN_FRONTEND=noninteractive

COPY dist/container/*.deb /tmp/aarnn/
COPY requirements.txt /tmp/aarnn-requirements.txt
COPY tools /app/tools
COPY scripts/container_entrypoint.sh /usr/local/bin/aarnn-entrypoint

RUN set -eux; \
    need_ui=0; \
    case ",${CARGO_FEATURES},${CONTAINER_WORKLOAD}," in \
        *,all,*|*,all-features,*|*,ui,*|*,image_input,*|*,video_input,*|*,webcam_input,*|*,robot_io,*|*,desktop_ui_workload,*|*,container,*|*,desktop-ui,*) need_ui=1 ;; \
    esac; \
    packages='ca-certificates python3 python3-venv python3-pip openmpi-bin ocl-icd-libopencl1'; \
    if [ "${need_ui}" = "1" ]; then \
        packages="$packages libgl1 libx11-6 libxext6 libxrender1 libice6 libsm6 libxcursor1 libxi6 libxrandr2 libxcomposite1 libxdamage1 libxfixes3 libxkbcommon0 libxkbcommon-x11-0 libasound2 libgtk-3-0"; \
    fi; \
    apt-get update; \
    apt-get install -y --no-install-recommends $packages; \
    arch="$(dpkg --print-architecture)"; \
    deb="$(find /tmp/aarnn -maxdepth 1 -type f -name "aarnn-rust_*_${arch}.deb" | head -n 1)"; \
    test -n "$deb"; \
    apt-get install -y --no-install-recommends "$deb"; \
    python3 -c "import os,sys; min_version=os.environ.get('PYTHON_MIN_VERSION','3.12'); min_tuple=tuple(int(x) for x in min_version.split('.')); sys.exit(0) if sys.version_info>=min_tuple else (_ for _ in ()).throw(SystemExit(f'Python {min_version}+ required, got {sys.version}'))"; \
    python3 -m venv /opt/aarnn-venv; \
    /opt/aarnn-venv/bin/pip install --no-cache-dir --upgrade pip setuptools wheel; \
    /opt/aarnn-venv/bin/pip install --no-cache-dir --default-timeout=1000 --retries 10 "numpy<2.0.0"; \
    /opt/aarnn-venv/bin/pip install --no-cache-dir --only-binary=PyQt6,PyQt6-Qt6,PyQt6-sip "PyQt6>=6.7.1"; \
    /opt/aarnn-venv/bin/pip install --no-cache-dir --default-timeout=3600 --retries 10 \
        --only-binary=onnx,onnxruntime,tflite-support,PyQt6,PyQt6-Qt6,PyQt6-sip \
        -r /tmp/aarnn-requirements.txt; \
    /opt/aarnn-venv/bin/python -m compileall -b -q /app/tools; \
    find /app/tools -type f -name '*.py' -delete; \
    find /app/tools -type d -name '__pycache__' -prune -exec rm -rf {} +; \
    chmod 0755 /usr/local/bin/aarnn-entrypoint; \
    mkdir -p /app/outputs /app/logs /app/data/runtime; \
    chmod 777 /app/outputs /app/logs /app/data/runtime; \
    ln -sf /usr/bin/aarnn_rust /app/aarnn_rust; \
    ln -sf /usr/bin/web_ui /app/web_ui; \
    ln -sf /usr/share/aarnn-rust/config.json /app/config.json; \
    rm -rf /tmp/aarnn /tmp/aarnn-requirements.txt /var/lib/apt/lists/*

WORKDIR /app

# Environment variables for hardware discovery and shared runtime defaults.
ENV PATH="/opt/aarnn-venv/bin:/usr/lib/x86_64-linux-gnu/openmpi/bin:/usr/lib/aarch64-linux-gnu/openmpi/bin:${PATH}"
ENV OCL_ICD_VENDORS=/etc/OpenCL/vendors

# OpenShift/Kubernetes security: run as a non-privileged user.
USER 1001

# Common ports used by the various workloads.
EXPOSE 50051
EXPOSE 50050/udp
EXPOSE 8080

LABEL io.k8s.description="Neuromorphic simulation and visualization engine" \
      io.k8s.display-name="AARNN" \
      io.openshift.expose-services="50051:grpc,8080:http" \
      io.openshift.tags="neuromorphic,ai,distributed,rust,web" \
      org.opencontainers.image.title="AARNN ${CONTAINER_WORKLOAD}" \
      org.opencontainers.image.description="Role-specific AARNN workload image (${CONTAINER_WORKLOAD})" \
      org.neuralmimicry.aarnn.workload="${CONTAINER_WORKLOAD}" \
      org.neuralmimicry.aarnn.features="${CARGO_FEATURES}"

ENTRYPOINT ["/usr/local/bin/aarnn-entrypoint"]
