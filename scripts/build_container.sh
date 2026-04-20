#!/bin/bash
set -euo pipefail

# Multi-arch Container Build Script for AARNN
# Requires: podman with qemu-user-static or docker buildx

IMAGE_NAME=${1:-"ghcr.io/neuralmimicry/aarnn_rust"}
REQUESTED_TAG=${2:-"engine"}
IMAGE_TAG="engine"
PUSH=${3:-"false"}
CARGO_FEATURES=${4:-"all"}
PYTHON_MIN_VERSION=${PYTHON_MIN_VERSION:-${5:-"3.12"}}
PYTHON_FULL_VERSION=${PYTHON_FULL_VERSION:-${6:-"3.12.2"}}
NO_CACHE=${NO_CACHE:-${7:-"false"}}
SKIP_REMOTE_MANIFEST=${SKIP_REMOTE_MANIFEST:-${8:-"false"}}

KNOWN_ARCHES=("amd64" "arm64")

# Ensure we're building from the engine branch
if ! command -v git &> /dev/null; then
    echo "Error: git is required to verify the current branch."
    exit 1
fi
CURRENT_BRANCH=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "")
if [ "${CURRENT_BRANCH}" != "engine" ]; then
    echo "Error: build must run from the 'engine' branch (current: '${CURRENT_BRANCH}')."
    exit 1
fi

if [ "${REQUESTED_TAG}" != "engine" ]; then
    echo "Error: tag must be 'engine' for this build (requested: '${REQUESTED_TAG}')."
    exit 1
fi

# Detect host platform (build native only)
HOST_UNAME=$(uname -m)
case "${HOST_UNAME}" in
    x86_64|amd64)
        HOST_ARCH="amd64"
        ;;
    aarch64|arm64)
        HOST_ARCH="arm64"
        ;;
    *)
        echo "Error: Unsupported host architecture '${HOST_UNAME}'."
        exit 1
        ;;
esac

HOST_PLATFORM="linux/${HOST_ARCH}"
ARCH_TAG="${IMAGE_TAG}-${HOST_ARCH}"

echo "Detected host platform: ${HOST_PLATFORM}"
echo "Building native image: ${IMAGE_NAME}:${ARCH_TAG} with features: ${CARGO_FEATURES}"
echo "Will assemble manifest: ${IMAGE_NAME}:${IMAGE_TAG} using any pre-existing platform images"
echo "Enforcing Python minimum version: ${PYTHON_MIN_VERSION}"
echo "Python full version: ${PYTHON_FULL_VERSION}"
NO_CACHE_ARG=""
case "${NO_CACHE}" in
    true|TRUE|1|yes|YES)
        NO_CACHE="true"
        ;;
    *)
        NO_CACHE="false"
        ;;
esac
if [ "${NO_CACHE}" == "true" ]; then
    NO_CACHE_ARG="--no-cache"
    echo "Build cache disabled."
fi
case "${SKIP_REMOTE_MANIFEST}" in
    true|TRUE|1|yes|YES)
        SKIP_REMOTE_MANIFEST="true"
        ;;
    *)
        SKIP_REMOTE_MANIFEST="false"
        ;;
esac
if [ "${SKIP_REMOTE_MANIFEST}" == "true" ]; then
    echo "Skipping remote manifest inspection."
fi

# Check for podman
if command -v podman &> /dev/null; then
    echo "Using Podman for build..."
    # Build only the native platform
    podman build ${NO_CACHE_ARG} --platform ${HOST_PLATFORM} -t ${IMAGE_NAME}:${ARCH_TAG} \
        --build-arg CARGO_FEATURES="${CARGO_FEATURES}" \
        --build-arg PYTHON_MIN_VERSION="${PYTHON_MIN_VERSION}" \
        --build-arg PYTHON_FULL_VERSION="${PYTHON_FULL_VERSION}" .

    # Create a manifest to hold the multi-arch images
    podman manifest rm ${IMAGE_NAME}:${IMAGE_TAG} 2>/dev/null || true
    podman manifest create ${IMAGE_NAME}:${IMAGE_TAG}
    podman manifest add ${IMAGE_NAME}:${IMAGE_TAG} ${IMAGE_NAME}:${ARCH_TAG}

    # Track which archs have been added already
    declare -A ADDED_ARCHES
    ADDED_ARCHES["${HOST_ARCH}"]=1

    # Try to reuse any existing manifest list from the registry
    if [ "${SKIP_REMOTE_MANIFEST}" != "true" ] && command -v python3 &> /dev/null; then
        tmpfile=$(mktemp)
        trap 'rm -f "${tmpfile}"' EXIT

        if podman manifest inspect docker://${IMAGE_NAME}:${IMAGE_TAG} > "${tmpfile}" 2>/dev/null; then
            while IFS=$'\t' read -r platform arch digest; do
                [ -z "${digest}" ] && continue
                if [ -n "${ADDED_ARCHES[${arch}]:-}" ]; then
                    continue
                fi
                echo "Adding existing ${platform} digest ${digest} to manifest..."
                if podman manifest add ${IMAGE_NAME}:${IMAGE_TAG} docker://${IMAGE_NAME}@${digest} 2>/dev/null; then
                    ADDED_ARCHES["${arch}"]=1
                else
                    echo "Warning: failed to add remote digest ${digest} (not accessible)."
                fi
            done < <(python3 - "${HOST_ARCH}" "${tmpfile}" <<'PY'
import json
import sys

host_arch = sys.argv[1]
path = sys.argv[2]
with open(path, "r", encoding="utf-8") as f:
    data = json.load(f)

manifests = data.get("manifests")
if not manifests:
    sys.exit(0)

for m in manifests:
    plat = m.get("platform", {})
    arch = plat.get("architecture", "")
    os_name = plat.get("os", "linux")
    variant = plat.get("variant", "")
    digest = m.get("digest", "")
    if not arch or not digest:
        continue
    if arch == host_arch:
        continue
    platform = f"{os_name}/{arch}"
    if variant:
        platform = f"{platform}/{variant}"
    print(f"{platform}\t{arch}\t{digest}")
PY
            )
        else
            echo "No existing remote manifest list found (or not accessible)."
        fi
    elif [ "${SKIP_REMOTE_MANIFEST}" != "true" ]; then
        echo "python3 not found; skipping remote manifest inspection."
    fi

    # Try to add arch-specific tags for other platforms, if they exist
    if [ "${SKIP_REMOTE_MANIFEST}" != "true" ]; then
        for arch in "${KNOWN_ARCHES[@]}"; do
            if [ -n "${ADDED_ARCHES[${arch}]:-}" ]; then
                continue
            fi
            other_tag="${IMAGE_NAME}:${IMAGE_TAG}-${arch}"
            if podman manifest inspect docker://${other_tag} >/dev/null 2>&1; then
                echo "Adding existing ${other_tag} to manifest..."
                if podman manifest add ${IMAGE_NAME}:${IMAGE_TAG} docker://${other_tag} 2>/dev/null; then
                    ADDED_ARCHES["${arch}"]=1
                else
                    echo "Warning: failed to add ${other_tag} (not accessible)."
                fi
            fi
        done
    fi

    echo "Multi-arch manifest assembled locally."
    if [ "$PUSH" == "true" ]; then
        echo "Pushing to registry..."
        podman push ${IMAGE_NAME}:${ARCH_TAG} docker://${IMAGE_NAME}:${ARCH_TAG}

        # Recreate a clean manifest list for push to avoid local manifest corruption.
        podman manifest rm ${IMAGE_NAME}:${IMAGE_TAG} 2>/dev/null || true
        podman manifest create ${IMAGE_NAME}:${IMAGE_TAG}
        podman manifest add ${IMAGE_NAME}:${IMAGE_TAG} ${IMAGE_NAME}:${ARCH_TAG}

        if [ "${SKIP_REMOTE_MANIFEST}" != "true" ]; then
            for arch in "${KNOWN_ARCHES[@]}"; do
                if [ "${arch}" == "${HOST_ARCH}" ]; then
                    continue
                fi
                other_tag="${IMAGE_NAME}:${IMAGE_TAG}-${arch}"
                if podman manifest inspect docker://${other_tag} >/dev/null 2>&1; then
                    echo "Adding existing ${other_tag} to manifest..."
                    if ! podman manifest add ${IMAGE_NAME}:${IMAGE_TAG} docker://${other_tag} 2>/dev/null; then
                        echo "Warning: failed to add ${other_tag} (not accessible)."
                    fi
                fi
            done
        fi

        podman manifest push ${IMAGE_NAME}:${IMAGE_TAG} docker://${IMAGE_NAME}:${IMAGE_TAG}
    else
        echo "To push native image: podman push ${IMAGE_NAME}:${ARCH_TAG} docker://${IMAGE_NAME}:${ARCH_TAG}"
        echo "To push manifest list: podman manifest push ${IMAGE_NAME}:${IMAGE_TAG} docker://${IMAGE_NAME}:${IMAGE_TAG}"
    fi

# Check for docker buildx
elif docker buildx version &> /dev/null; then
    echo "Using Docker Buildx for build..."
    if [ "$PUSH" == "true" ]; then
        docker buildx build ${NO_CACHE_ARG} --platform ${HOST_PLATFORM} -t ${IMAGE_NAME}:${ARCH_TAG} \
            --build-arg CARGO_FEATURES="${CARGO_FEATURES}" \
            --build-arg PYTHON_MIN_VERSION="${PYTHON_MIN_VERSION}" \
            --build-arg PYTHON_FULL_VERSION="${PYTHON_FULL_VERSION}" . --push
    else
        docker buildx build ${NO_CACHE_ARG} --platform ${HOST_PLATFORM} -t ${IMAGE_NAME}:${ARCH_TAG} \
            --build-arg CARGO_FEATURES="${CARGO_FEATURES}" \
            --build-arg PYTHON_MIN_VERSION="${PYTHON_MIN_VERSION}" \
            --build-arg PYTHON_FULL_VERSION="${PYTHON_FULL_VERSION}" . --load
    fi
    echo "Native image build complete."
    echo "Note: Manifest assembly from pre-existing platforms is implemented for Podman only."

else
    echo "Error: Neither podman nor docker buildx found. Please install one of them."
    exit 1
fi
