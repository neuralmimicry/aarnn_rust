ARG TARGET_PAGE_SIZE=4k
FROM cyberbotics/webots.cloud:R2022b-ubuntu20.04
ARG PROJECT_PATH=webots-project
ARG TARGET_PAGE_SIZE
LABEL org.opencontainers.image.page-size="${TARGET_PAGE_SIZE}"
RUN mkdir -p "$PROJECT_PATH"
COPY . "$PROJECT_PATH"
