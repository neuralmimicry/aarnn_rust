#!/bin/bash
set -euo pipefail

IMAGE_NAME=${1:-"ghcr.io/neuralmimicry/aarnn_rust"}
IMAGE_TAG=${2:-"brainregions"}
PUSH=${3:-"false"}

KNOWN_ARCHES=("amd64" "arm64")

HOST_UNAME=$(uname -m)
case "${HOST_UNAME}" in
    x86_64|amd64)
        HOST_ARCH="amd64"
        ;;
    aarch64|arm64)
        HOST_ARCH="arm64"
        ;;
    *)
        echo "Unsupported host architecture ${HOST_UNAME}"
        exit 1
        ;;
esac

HOST_PLATFORM="linux/${HOST_ARCH}"
ARCH_TAG="${IMAGE_TAG}-${HOST_ARCH}"

echo "Host architecture: ${HOST_ARCH}"
echo "Building ${IMAGE_NAME}:${ARCH_TAG}"

if command -v podman >/dev/null 2>&1; then

    podman build         --platform ${HOST_PLATFORM}         -t ${IMAGE_NAME}:${ARCH_TAG}         --build-arg CARGO_FEATURES=all         --build-arg PYTHON_MIN_VERSION=3.12         --build-arg PYTHON_FULL_VERSION=3.12.2         -f Containerfile .

    podman manifest rm ${IMAGE_NAME}:${IMAGE_TAG} 2>/dev/null || true
    podman manifest create ${IMAGE_NAME}:${IMAGE_TAG}

    echo "Adding locally built architecture"
    podman manifest add ${IMAGE_NAME}:${IMAGE_TAG} ${IMAGE_NAME}:${ARCH_TAG}

    echo "Attempting to reuse CI-built images from registry..."

    for arch in "${KNOWN_ARCHES[@]}"; do
        if [ "${arch}" == "${HOST_ARCH}" ]; then
            continue
        fi

        REMOTE_TAG="${IMAGE_NAME}:${IMAGE_TAG}-${arch}"

        if podman manifest inspect docker://${REMOTE_TAG} >/dev/null 2>&1; then
            echo "Reusing CI-built image ${REMOTE_TAG}"
            podman manifest add ${IMAGE_NAME}:${IMAGE_TAG} docker://${REMOTE_TAG}
        else
            echo "No remote image found for ${REMOTE_TAG}"
        fi
    done

    if [ "${PUSH}" == "true" ]; then
        echo "Pushing architecture image"
        podman push ${IMAGE_NAME}:${ARCH_TAG}

        echo "Pushing assembled manifest"
        podman manifest push ${IMAGE_NAME}:${IMAGE_TAG} docker://${IMAGE_NAME}:${IMAGE_TAG}
    else
        echo "Local manifest assembled. Use PUSH=true to publish."
    fi

else
    echo "Podman required for manifest reuse workflow."
    exit 1
fi
