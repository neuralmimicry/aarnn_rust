# Multi-arch Containerfile for AARNN
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
ENV PYTHON_MIN_VERSION=${PYTHON_MIN_VERSION}
ENV PYTHON_FULL_VERSION=${PYTHON_FULL_VERSION}

# Install build-time dependencies
# Note: CentOS Stream 9 is used to access the full set of dependencies (AppStream, CRB)
# which are restricted in the minimal UBI images without a subscription.
RUN dnf install -y 'dnf-command(config-manager)' && \
    dnf config-manager --set-enabled crb && \
    dnf install -y \
    gcc gcc-c++ make cmake wget unzip \
    openssl-devel pkgconfig \
    fontconfig-devel freetype-devel \
    clang-devel llvm-devel \
    libjpeg-turbo-devel libpng-devel libtiff-devel \
    protobuf-compiler libnl3-devel \
    opencl-headers ocl-icd-devel \
    libibverbs-devel \
    alsa-lib-devel libX11-devel libXcursor-devel libXi-devel libXrandr-devel \
    libXcomposite-devel libXdamage-devel libXfixes-devel libXext-devel \
    libXrender-devel mesa-libGL-devel gtk3-devel libxkbcommon-devel wayland-devel \
    git \
    && dnf clean all

# Bring in Python 3.12 built from source
COPY --from=python_builder /opt/python3.12 /opt/python3.12
RUN ln -sf /opt/python3.12/bin/python3.12 /usr/bin/python3.12 && \
    ln -sf /opt/python3.12/bin/python3.12 /usr/bin/python3 && \
    ln -sf /opt/python3.12/bin/pip3.12 /usr/bin/pip3 && \
    python3 -c "import os,sys; min_version=os.environ.get('PYTHON_MIN_VERSION','3.12'); min_tuple=tuple(int(x) for x in min_version.split('.')); sys.exit(0) if sys.version_info>=min_tuple else (_ for _ in ()).throw(SystemExit(f'Python {min_version}+ required, got {sys.version}'))"

# Build OpenCV from source (it's missing from CentOS 9 repos)
WORKDIR /tmp/opencv_build
RUN git clone --depth 1 -b 4.x https://github.com/opencv/opencv.git && \
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
    make -j$(nproc) && \
    make install && \
    rm -rf /tmp/opencv_build

# Set environment for Rust build to find OpenCV
ENV PKG_CONFIG_PATH="/usr/local/lib64/pkgconfig:/usr/local/lib/pkgconfig"
ENV LD_LIBRARY_PATH="/usr/local/lib64:/usr/local/lib"

# Install Rust toolchain
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /build

# Copy source code and build dependencies
# Note: third_party is required because Cargo.toml patches ibverbs-sys to a local path.
COPY Cargo.toml Cargo.lock config.json ./
COPY proto ./proto
COPY tools ./tools
COPY build.rs ./
COPY src ./src
COPY web_ui ./web_ui
COPY third_party ./third_party

# Features to enable during build (can be overridden via --build-arg)
# Use CARGO_FEATURES="all" or "all-features" to build with --all-features.
ARG CARGO_FEATURES="all"
RUN if [ "${CARGO_FEATURES}" = "all" ] || [ "${CARGO_FEATURES}" = "all-features" ]; then \
        cargo build --release --all-features --bin aarnn_rust --bin web_ui; \
    else \
        cargo build --release --features "${CARGO_FEATURES}" --bin aarnn_rust --bin web_ui; \
    fi

# --- Stage 2: Runtime ---
FROM quay.io/centos/centos:stream9
ARG PYTHON_MIN_VERSION=3.12
ARG PYTHON_FULL_VERSION=3.12.2
ENV PYTHON_MIN_VERSION=${PYTHON_MIN_VERSION}
ENV PYTHON_FULL_VERSION=${PYTHON_FULL_VERSION}

# Install runtime dependencies
RUN dnf install -y \
    openssl \
    libffi bzip2-libs zlib xz-libs readline sqlite-libs ncurses-libs gdbm-libs libuuid \
    fontconfig freetype \
    libjpeg-turbo libpng libtiff \
    libibverbs libnl3 \
    ocl-icd \
    mesa-libGL \
    libX11 libXext libXrender libICE libSM libXcursor libXi libXrandr \
    libXcomposite libXdamage libXfixes libxkbcommon libxkbcommon-x11 alsa-lib gtk3 \
    && dnf clean all

# Bring in Python 3.12 built from source
COPY --from=python_builder /opt/python3.12 /opt/python3.12
RUN ln -sf /opt/python3.12/bin/python3.12 /usr/bin/python3.12 && \
    ln -sf /opt/python3.12/bin/python3.12 /usr/bin/python3 && \
    ln -sf /opt/python3.12/bin/pip3.12 /usr/bin/pip3 && \
    python3 -c "import os,sys; min_version=os.environ.get('PYTHON_MIN_VERSION','3.12'); min_tuple=tuple(int(x) for x in min_version.split('.')); sys.exit(0) if sys.version_info>=min_tuple else (_ for _ in ()).throw(SystemExit(f'Python {min_version}+ required, got {sys.version}'))"

# Copy OpenCV libraries from builder
COPY --from=builder /usr/local/lib64/ /usr/local/lib64/
COPY --from=builder /usr/local/lib/ /usr/local/lib/
COPY --from=builder /usr/local/include/opencv4/ /usr/local/include/opencv4/
COPY --from=builder /usr/local/share/opencv4/ /usr/local/share/opencv4/

# Install Python requirements for tools
COPY requirements.txt .
RUN python3.12 -m ensurepip --upgrade && \
    python3.12 -m pip install --no-cache-dir --upgrade --ignore-installed setuptools wheel
RUN python3.12 -m pip install --no-cache-dir --default-timeout=1000 --retries 10 "numpy<2.0.0"
RUN python3.12 -m pip install --no-cache-dir --only-binary=PyQt6,PyQt6-Qt6,PyQt6-sip "PyQt6>=6.7.1"
RUN python3.12 -m pip install --no-cache-dir --default-timeout=3600 --retries 10 \
                 --only-binary=onnx,onnxruntime,tflite-support,PyQt6,PyQt6-Qt6,PyQt6-sip \
                 -r requirements.txt

WORKDIR /app

# Copy the compiled binary and necessary assets from the builder stage
COPY --from=builder /build/target/release/aarnn_rust .
COPY --from=builder /build/target/release/web_ui .
COPY --from=builder /build/config.json .
COPY --from=builder /build/tools ./tools
COPY --from=builder /build/proto ./proto

# Create directories for outputs (PNGs, logs)
RUN mkdir -p /app/outputs && chmod 777 /app/outputs

# Environment variables for Hardware Discovery
# - LD_LIBRARY_PATH: Ensure libraries like libopencv are found
# - NM_LOG_DIR: Custom env var if supported, otherwise use default
ENV LD_LIBRARY_PATH="/usr/local/lib64:/usr/local/lib:/usr/lib64"
ENV OCL_ICD_VENDORS=/etc/OpenCL/vendors

# OpenShift/Kubernetes security: Run as a non-privileged user
USER 1001

# Default gRPC port for Distributed Mode
EXPOSE 50051
# Default UDP Discovery port
EXPOSE 50050/udp
# Web UI HTTP port
EXPOSE 8080

# Labels for OpenShift
LABEL io.k8s.description="Neuromorphic simulation and visualization engine" \
      io.k8s.display-name="AARNN" \
      io.openshift.expose-services="50051:grpc,8080:http" \
      io.openshift.tags="neuromorphic,ai,distributed,rust,web"

# Default entrypoint runs the help command to show available modes
ENTRYPOINT ["./aarnn_rust"]
CMD ["--help"]
