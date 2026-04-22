# Multi-arch Containerfile for AARNN role-specific workloads.
# Target Architectures: linux/amd64, linux/arm64
# Base: CentOS Stream 9

# --- Stage 0: Python 3.12 Builder ---
ARG PYTHON_MIN_VERSION=3.12
ARG PYTHON_FULL_VERSION=3.12.2
FROM quay.io/centos/centos:stream9 AS python_builder
ARG PYTHON_FULL_VERSION=3.12.2
RUN dnf install -y 'dnf-command(config-manager)' && \
    dnf config-manager --set-enabled crb && \
    dnf install -y \
    gcc make wget tar \
    openssl-devel bzip2-devel libffi-devel zlib-devel xz-devel \
    readline-devel sqlite-devel ncurses-devel gdbm-devel libuuid-devel \
    && dnf clean all
WORKDIR /tmp/python_build
RUN wget -q "https://www.python.org/ftp/python/${PYTHON_FULL_VERSION}/Python-${PYTHON_FULL_VERSION}.tgz" -O python.tgz && \
    tar -xzf python.tgz && \
    cd "Python-${PYTHON_FULL_VERSION}" && \
    ./configure --prefix=/opt/python3.12 --with-ensurepip=install && \
    make -j"$(nproc)" && \
    make altinstall

# --- Stage 1: Build ---
ARG PYTHON_MIN_VERSION=3.12
ARG PYTHON_FULL_VERSION=3.12.2
FROM quay.io/centos/centos:stream9 AS builder
ARG PYTHON_MIN_VERSION=3.12
ARG PYTHON_FULL_VERSION=3.12.2
ARG CONTAINER_WORKLOAD="standalone"
ARG CARGO_FEATURES="standalone_workload"
ARG CARGO_BUILD_TARGETS="aarnn_rust"
ENV PYTHON_MIN_VERSION=${PYTHON_MIN_VERSION}
ENV PYTHON_FULL_VERSION=${PYTHON_FULL_VERSION}
ENV CONTAINER_WORKLOAD=${CONTAINER_WORKLOAD}
ENV CARGO_FEATURES=${CARGO_FEATURES}
ENV CARGO_BUILD_TARGETS=${CARGO_BUILD_TARGETS}

# Install build-time dependencies.
RUN need_ui=0 && \
    case ",${CARGO_FEATURES},${CONTAINER_WORKLOAD}," in \
        *,all,*|*,all-features,*|*,ui,*|*,image_input,*|*,video_input,*|*,webcam_input,*|*,robot_io,*|*,desktop_ui_workload,*|*,container,*|*,desktop-ui,*) need_ui=1 ;; \
    esac && \
    dnf install -y 'dnf-command(config-manager)' && \
    dnf config-manager --set-enabled crb && \
    packages='gcc gcc-c++ make cmake wget unzip openssl-devel pkgconfig libffi-devel openblas-devel systemd-devel fontconfig-devel freetype-devel clang-devel llvm-devel libjpeg-turbo-devel libpng-devel libtiff-devel protobuf-compiler libnl3-devel opencl-headers ocl-icd-devel libibverbs-devel openmpi openmpi-devel libgomp git' && \
    if [ "${need_ui}" = "1" ]; then \
        packages="$packages alsa-lib-devel libX11-devel libXcursor-devel libXi-devel libXrandr-devel libXcomposite-devel libXdamage-devel libXfixes-devel libXext-devel libXrender-devel mesa-libGL-devel gtk3-devel libxkbcommon-devel wayland-devel"; \
    fi && \
    dnf install -y $packages && \
    dnf clean all

# Bring in Python 3.12 built from source.
COPY --from=python_builder /opt/python3.12 /opt/python3.12
RUN ln -sf /opt/python3.12/bin/python3.12 /usr/bin/python3.12 && \
    ln -sf /opt/python3.12/bin/python3.12 /usr/bin/python3 && \
    ln -sf /opt/python3.12/bin/pip3.12 /usr/bin/pip3 && \
    python3 -c "import os,sys; min_version=os.environ.get('PYTHON_MIN_VERSION','3.12'); min_tuple=tuple(int(x) for x in min_version.split('.')); sys.exit(0) if sys.version_info>=min_tuple else (_ for _ in ()).throw(SystemExit(f'Python {min_version}+ required, got {sys.version}'))"

# Build OpenCV from source only when the requested feature set needs it.
# arm64 builds intentionally skip it because the crate graph does not pull
# `opencv` there, even with `--all-features`.
WORKDIR /tmp/opencv_build
RUN mkdir -p /usr/local/lib64 /usr/local/lib /usr/local/include/opencv4 /usr/local/share/opencv4 && \
    arch="$(uname -m)" && \
    need_opencv=0 && \
    case ",${CARGO_FEATURES}," in \
        *,all,*|*,all-features,*|*,video_input,*) need_opencv=1 ;; \
    esac && \
    if [ "${arch}" = "aarch64" ]; then \
        need_opencv=0; \
    fi && \
    if [ "${need_opencv}" = "1" ]; then \
        git clone --depth 1 -b 4.x https://github.com/opencv/opencv.git && \
        mkdir build && cd build && \
        cmake -D CMAKE_BUILD_TYPE=RELEASE \
              -D CMAKE_INSTALL_PREFIX=/usr/local \
              -D INSTALL_C_EXAMPLES=OFF \
              -D INSTALL_PYTHON_EXAMPLES=OFF \
              -D OPENCV_GENERATE_PKGCONFIG=ON \
              -D BUILD_EXAMPLES=OFF \
              -D BUILD_TESTS=OFF \
              -D BUILD_PERF_TESTS=OFF \
              -D BUILD_opencv_java=OFF \
              -D BUILD_opencv_python2=OFF \
              -D BUILD_opencv_python3=OFF \
              ../opencv && \
        make -j"$(nproc)" && \
        make install; \
    else \
        echo "Skipping OpenCV build for ${arch} with CARGO_FEATURES=${CARGO_FEATURES}"; \
    fi && \
    rm -rf /tmp/opencv_build

# Set environment for Rust build to find OpenCV + MPI.
ENV PATH="/usr/lib64/openmpi/bin:${PATH}"
ENV MPI_PKG_CONFIG="ompi"
ENV PKG_CONFIG_PATH="/usr/lib64/openmpi/lib/pkgconfig:/usr/local/lib64/pkgconfig:/usr/local/lib/pkgconfig:/usr/lib64/pkgconfig"
ENV LD_LIBRARY_PATH="/usr/lib64/openmpi/lib:/usr/local/lib64:/usr/local/lib"

# Install Rust toolchain.
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /build

# Copy source code and build dependencies.
# Note: third_party is required because Cargo.toml patches ibverbs-sys to a local path.
COPY Cargo.toml Cargo.lock config.json ./
COPY proto ./proto
COPY tools ./tools
COPY build.rs ./
COPY src ./src
COPY web_ui ./web_ui
COPY third_party ./third_party

# Build only the workload-specific binaries with the matching feature bundle.
RUN build_targets='' && \
    for bin in ${CARGO_BUILD_TARGETS}; do \
        build_targets="${build_targets} --bin ${bin}"; \
    done && \
    if [ -z "${build_targets}" ]; then \
        echo "CARGO_BUILD_TARGETS must not be empty" >&2; \
        exit 1; \
    fi && \
    if [ -z "${CARGO_FEATURES}" ] || [ "${CARGO_FEATURES}" = "default" ]; then \
        cargo build --locked --release ${build_targets}; \
    elif [ "${CARGO_FEATURES}" = "all" ] || [ "${CARGO_FEATURES}" = "all-features" ]; then \
        cargo build --locked --release --all-features ${build_targets}; \
    else \
        cargo build --locked --release --no-default-features --features "${CARGO_FEATURES}" ${build_targets}; \
    fi && \
    mkdir -p /tmp/container-artifacts/bin && \
    for bin in ${CARGO_BUILD_TARGETS}; do \
        install -m 0755 "target/release/${bin}" "/tmp/container-artifacts/bin/${bin}"; \
    done

RUN python3.12 -m compileall -b -q /build/tools \
    && find /build/tools -type f -name '*.py' -delete \
    && find /build/tools -type d -name '__pycache__' -prune -exec rm -rf {} +

# --- Stage 2: Runtime ---
FROM quay.io/centos/centos:stream9
ARG PYTHON_MIN_VERSION=3.12
ARG PYTHON_FULL_VERSION=3.12.2
ARG CONTAINER_WORKLOAD="standalone"
ARG CARGO_FEATURES="standalone_workload"
ENV PYTHON_MIN_VERSION=${PYTHON_MIN_VERSION}
ENV PYTHON_FULL_VERSION=${PYTHON_FULL_VERSION}
ENV AARNN_CONTAINER_WORKLOAD=${CONTAINER_WORKLOAD}

# Install runtime dependencies.
RUN need_ui=0 && \
    case ",${CARGO_FEATURES},${CONTAINER_WORKLOAD}," in \
        *,all,*|*,all-features,*|*,ui,*|*,image_input,*|*,video_input,*|*,webcam_input,*|*,robot_io,*|*,desktop_ui_workload,*|*,container,*|*,desktop-ui,*) need_ui=1 ;; \
    esac && \
    packages='openssl libffi bzip2-libs zlib xz-libs readline sqlite-libs ncurses-libs gdbm-libs libuuid openblas fontconfig freetype libjpeg-turbo libpng libtiff libibverbs libnl3 openmpi libgomp ocl-icd' && \
    if [ "${need_ui}" = "1" ]; then \
        packages="$packages mesa-libGL libX11 libXext libXrender libICE libSM libXcursor libXi libXrandr libXcomposite libXdamage libXfixes libxkbcommon libxkbcommon-x11 alsa-lib gtk3"; \
    fi && \
    dnf install -y $packages && \
    dnf clean all

# Bring in Python 3.12 built from source.
COPY --from=python_builder /opt/python3.12 /opt/python3.12
RUN ln -sf /opt/python3.12/bin/python3.12 /usr/bin/python3.12 && \
    ln -sf /opt/python3.12/bin/python3.12 /usr/bin/python3 && \
    ln -sf /opt/python3.12/bin/pip3.12 /usr/bin/pip3 && \
    python3 -c "import os,sys; min_version=os.environ.get('PYTHON_MIN_VERSION','3.12'); min_tuple=tuple(int(x) for x in min_version.split('.')); sys.exit(0) if sys.version_info>=min_tuple else (_ for _ in ()).throw(SystemExit(f'Python {min_version}+ required, got {sys.version}'))"

# Copy OpenCV libraries from builder when present.
COPY --from=builder /usr/local/lib64/ /usr/local/lib64/
COPY --from=builder /usr/local/lib/ /usr/local/lib/
COPY --from=builder /usr/local/include/opencv4/ /usr/local/include/opencv4/
COPY --from=builder /usr/local/share/opencv4/ /usr/local/share/opencv4/

# Install Python requirements for tools.
COPY requirements.txt .
RUN python3.12 -m ensurepip --upgrade && \
    python3.12 -m pip install --no-cache-dir --upgrade --ignore-installed setuptools wheel
RUN python3.12 -m pip install --no-cache-dir --default-timeout=1000 --retries 10 "numpy<2.0.0"
RUN python3.12 -m pip install --no-cache-dir --only-binary=PyQt6,PyQt6-Qt6,PyQt6-sip "PyQt6>=6.7.1"
RUN python3.12 -m pip install --no-cache-dir --default-timeout=3600 --retries 10 \
                 --only-binary=onnx,onnxruntime,tflite-support,PyQt6,PyQt6-Qt6,PyQt6-sip \
                 -r requirements.txt

WORKDIR /app

# Copy the workload binaries and runtime assets.
COPY --from=builder /tmp/container-artifacts/bin/ /app/
COPY --from=builder /build/config.json ./
COPY --from=builder /build/tools ./tools
COPY scripts/container_entrypoint.sh /usr/local/bin/aarnn-entrypoint
RUN chmod 0755 /usr/local/bin/aarnn-entrypoint

# Create directories for outputs and logs.
RUN mkdir -p /app/outputs /app/logs && chmod 777 /app/outputs /app/logs

# Environment variables for hardware discovery and shared runtime defaults.
ENV PATH="/usr/lib64/openmpi/bin:${PATH}"
ENV LD_LIBRARY_PATH="/usr/lib64/openmpi/lib:/usr/local/lib64:/usr/local/lib:/usr/lib64"
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
