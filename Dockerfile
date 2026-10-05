ARG TARGET_PAGE_SIZE=4k
FROM cyberbotics/webots.cloud:R2022b-ubuntu20.04
ARG PROJECT_PATH=webots-project
ARG TARGET_PAGE_SIZE
LABEL org.opencontainers.image.page-size="${TARGET_PAGE_SIZE}"
RUN mkdir -p "$PROJECT_PATH"
COPY . "$PROJECT_PATH"

# OCI metadata (final stage) so GHCR links the package to its source repository.
LABEL org.opencontainers.image.source="https://github.com/neuralmimicry/aarnn_rust" \
      org.opencontainers.image.url="https://github.com/neuralmimicry/aarnn_rust" \
      org.opencontainers.image.description="Webots cloud simulation project image for AARNN embodied simulation" \
      org.opencontainers.image.vendor="NeuralMimicry"
