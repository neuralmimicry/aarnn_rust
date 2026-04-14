FROM cyberbotics/webots.cloud:R2022b-ubuntu20.04
ARG PROJECT_PATH=webots-project
RUN mkdir -p "$PROJECT_PATH"
COPY . "$PROJECT_PATH"
