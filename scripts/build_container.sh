#!/usr/bin/env bash
set -euo pipefail

# Role-specific multi-arch container build script for AARNN.
# Requires: podman or docker buildx.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
# shellcheck source=scripts/container_workloads.sh
source "${SCRIPT_DIR}/container_workloads.sh"

IMAGE_NAME=${1:-"ghcr.io/neuralmimicry/aarnn_rust"}
IMAGE_TAG=${2:-"engine"}
PUSH=${3:-"false"}
WORKLOADS_CSV=${WORKLOADS:-${4:-"standalone,orchestrator,node,web-ui,desktop-ui"}}
PYTHON_MIN_VERSION=${PYTHON_MIN_VERSION:-${5:-"3.12"}}
PYTHON_FULL_VERSION=${PYTHON_FULL_VERSION:-${6:-"3.12.2"}}
NO_CACHE=${NO_CACHE:-${7:-"false"}}
SKIP_REMOTE_MANIFEST=${SKIP_REMOTE_MANIFEST:-${8:-"false"}}
PULL=${PULL:-${9:-"false"}}
BUILD_TOOL=${CONTAINER_BUILD_TOOL:-${BUILD_TOOL:-""}}

KNOWN_ARCHES=("amd64" "arm64")
WORKLOADS=()

normalize_bool() {
    case "$1" in
        true|TRUE|1|yes|YES) printf '%s' 'true' ;;
        *) printf '%s' 'false' ;;
    esac
}

trim_csv_field() {
    local value="$1"
    value="${value#"${value%%[![:space:]]*}"}"
    value="${value%"${value##*[![:space:]]}"}"
    printf '%s' "$value"
}

parse_workloads() {
    local csv="$1"
    local item=""
    local -A seen=()
    local -a parsed=()

    if [ "$csv" = "all" ]; then
        mapfile -t WORKLOADS < <(aarnn_container_workload_names)
        return 0
    fi

    IFS=',' read -r -a parsed <<<"$csv"
    if [ "${#parsed[@]}" -eq 0 ]; then
        echo "No workloads specified." >&2
        exit 1
    fi

    for item in "${parsed[@]}"; do
        item="$(trim_csv_field "$item")"
        [ -n "$item" ] || continue
        aarnn_container_validate_workload "$item"
        if [ -n "${seen[$item]:-}" ]; then
            continue
        fi
        seen[$item]=1
        WORKLOADS+=("$item")
    done

    if [ "${#WORKLOADS[@]}" -eq 0 ]; then
        echo "No valid workloads specified in WORKLOADS_CSV='$csv'." >&2
        exit 1
    fi
}

detect_host_arch() {
    case "$(uname -m)" in
        x86_64|amd64) HOST_ARCH="amd64" ;;
        aarch64|arm64) HOST_ARCH="arm64" ;;
        *)
            echo "Error: unsupported host architecture '$(uname -m)'." >&2
            exit 1
            ;;
    esac
    HOST_PLATFORM="linux/${HOST_ARCH}"
}

role_tag_for() {
    local workload="$1"
    aarnn_container_workload_tag "$IMAGE_TAG" "$workload"
}

arch_tag_for() {
    local workload="$1"
    printf '%s-%s' "$(role_tag_for "$workload")" "$HOST_ARCH"
}

maybe_add_remote_manifest_entries_podman() {
    local manifest_ref="$1"
    declare -n added_arches_ref="$2"
    local tmpfile=""
    local platform=""
    local arch=""
    local digest=""

    if [ "$SKIP_REMOTE_MANIFEST" = "true" ]; then
        return 0
    fi
    if ! command -v python3 >/dev/null 2>&1; then
        echo "python3 not found; skipping remote manifest inspection for ${manifest_ref}."
        return 0
    fi

    tmpfile="$(mktemp)"
    if podman manifest inspect "docker://${manifest_ref}" >"${tmpfile}" 2>/dev/null; then
        while IFS=$'\t' read -r platform arch digest; do
            [ -n "$digest" ] || continue
            if [ -n "${added_arches_ref[$arch]:-}" ]; then
                continue
            fi
            echo "Adding existing ${platform} digest ${digest} to ${manifest_ref}..."
            if podman manifest add "${manifest_ref}" "docker://${IMAGE_NAME}@${digest}" >/dev/null 2>&1; then
                added_arches_ref["$arch"]=1
            else
                echo "Warning: failed to add remote digest ${digest}."
            fi
        done < <(python3 - "${HOST_ARCH}" "${tmpfile}" <<'PY'
import json
import sys

host_arch = sys.argv[1]
path = sys.argv[2]
with open(path, 'r', encoding='utf-8') as f:
    data = json.load(f)

for manifest in data.get('manifests') or []:
    plat = manifest.get('platform', {})
    arch = plat.get('architecture', '')
    os_name = plat.get('os', 'linux')
    variant = plat.get('variant', '')
    digest = manifest.get('digest', '')
    if not arch or not digest or arch == host_arch:
        continue
    platform = f"{os_name}/{arch}"
    if variant:
        platform = f"{platform}/{variant}"
    print(f"{platform}\t{arch}\t{digest}")
PY
        )
    else
        echo "No existing remote manifest list found for ${manifest_ref}."
    fi
    rm -f "${tmpfile}"
}

maybe_add_remote_arch_tags_podman() {
    local manifest_ref="$1"
    local role_tag="$2"
    declare -n added_arches_ref="$3"
    local arch=""
    local other_ref=""

    if [ "$SKIP_REMOTE_MANIFEST" = "true" ]; then
        return 0
    fi

    for arch in "${KNOWN_ARCHES[@]}"; do
        if [ -n "${added_arches_ref[$arch]:-}" ]; then
            continue
        fi
        other_ref="${IMAGE_NAME}:${role_tag}-${arch}"
        if podman manifest inspect "docker://${other_ref}" >/dev/null 2>&1; then
            echo "Adding existing ${other_ref} to ${manifest_ref}..."
            if podman manifest add "${manifest_ref}" "docker://${other_ref}" >/dev/null 2>&1; then
                added_arches_ref["$arch"]=1
            else
                echo "Warning: failed to add ${other_ref}."
            fi
        fi
    done
}

assemble_podman_manifest() {
    local workload="$1"
    local role_tag="$(role_tag_for "$workload")"
    local arch_tag="$(arch_tag_for "$workload")"
    local manifest_ref="${IMAGE_NAME}:${role_tag}"
    local native_ref="${IMAGE_NAME}:${arch_tag}"
    local -A added_arches=()

    podman manifest rm "${manifest_ref}" >/dev/null 2>&1 || true
    podman manifest create "${manifest_ref}" >/dev/null
    podman manifest add "${manifest_ref}" "${native_ref}" >/dev/null
    added_arches["${HOST_ARCH}"]=1

    maybe_add_remote_manifest_entries_podman "${manifest_ref}" added_arches
    maybe_add_remote_arch_tags_podman "${manifest_ref}" "${role_tag}" added_arches

    echo "Assembled manifest ${manifest_ref}."
}

build_workload_with_podman() {
    local workload="$1"
    local role_tag="$(role_tag_for "$workload")"
    local arch_tag="$(arch_tag_for "$workload")"
    local image_ref="${IMAGE_NAME}:${arch_tag}"
    local features="$(aarnn_container_workload_features "$workload")"
    local targets="$(aarnn_container_workload_targets "$workload")"
    local need_ui="$(aarnn_container_workload_needs_native_ui "$workload")"

    echo "Building ${image_ref}"
    echo "  workload: ${workload}"
    echo "  features: ${features}"
    echo "  targets: ${targets}"

    podman build ${NO_CACHE_ARG} --platform "${HOST_PLATFORM}" \
        ${PULL_ARG} \
        -t "${image_ref}" \
        --build-arg CONTAINER_WORKLOAD="${workload}" \
        --build-arg CARGO_FEATURES="${features}" \
        --build-arg CARGO_BUILD_TARGETS="${targets}" \
        --build-arg NEED_NATIVE_UI_RUNTIME="${need_ui}" \
        --build-arg PYTHON_MIN_VERSION="${PYTHON_MIN_VERSION}" \
        --build-arg PYTHON_FULL_VERSION="${PYTHON_FULL_VERSION}" \
        -f "${ROOT_DIR}/Containerfile" "${ROOT_DIR}"
}

push_workload_with_podman() {
    local workload="$1"
    local role_tag="$(role_tag_for "$workload")"
    local arch_tag="$(arch_tag_for "$workload")"
    local manifest_ref="${IMAGE_NAME}:${role_tag}"
    local native_ref="${IMAGE_NAME}:${arch_tag}"

    echo "Pushing ${native_ref}"
    podman push "${native_ref}" "docker://${native_ref}"
    assemble_podman_manifest "$workload"
    echo "Pushing ${manifest_ref}"
    podman manifest push "${manifest_ref}" "docker://${manifest_ref}"
}

build_workload_with_buildx() {
    local workload="$1"
    local arch_tag="$(arch_tag_for "$workload")"
    local image_ref="${IMAGE_NAME}:${arch_tag}"
    local features="$(aarnn_container_workload_features "$workload")"
    local targets="$(aarnn_container_workload_targets "$workload")"
    local need_ui="$(aarnn_container_workload_needs_native_ui "$workload")"
    local output_flag="--load"

    if [ "$PUSH" = "true" ]; then
        output_flag="--push"
    fi

    echo "Building ${image_ref} with Docker Buildx"
    docker buildx build ${NO_CACHE_ARG} --platform "${HOST_PLATFORM}" \
        ${PULL_ARG} \
        -t "${image_ref}" \
        --build-arg CONTAINER_WORKLOAD="${workload}" \
        --build-arg CARGO_FEATURES="${features}" \
        --build-arg CARGO_BUILD_TARGETS="${targets}" \
        --build-arg NEED_NATIVE_UI_RUNTIME="${need_ui}" \
        --build-arg PYTHON_MIN_VERSION="${PYTHON_MIN_VERSION}" \
        --build-arg PYTHON_FULL_VERSION="${PYTHON_FULL_VERSION}" \
        -f "${ROOT_DIR}/Containerfile" "${ROOT_DIR}" ${output_flag}
}

print_summary() {
    local workload=""
    local role_tag=""
    echo "Host platform: ${HOST_PLATFORM}"
    echo "Image repo: ${IMAGE_NAME}"
    echo "Base tag: ${IMAGE_TAG}"
    echo "Workloads: ${WORKLOADS[*]}"
    echo "Python minimum: ${PYTHON_MIN_VERSION}"
    echo "Python full: ${PYTHON_FULL_VERSION}"
    for workload in "${WORKLOADS[@]}"; do
        role_tag="$(role_tag_for "$workload")"
        echo "  - ${workload}: ${IMAGE_NAME}:${role_tag}-${HOST_ARCH} (manifest ${IMAGE_NAME}:${role_tag})"
    done
}

parse_workloads "$WORKLOADS_CSV"
detect_host_arch
NO_CACHE="$(normalize_bool "$NO_CACHE")"
SKIP_REMOTE_MANIFEST="$(normalize_bool "$SKIP_REMOTE_MANIFEST")"
PUSH="$(normalize_bool "$PUSH")"
PULL="$(normalize_bool "$PULL")"
NO_CACHE_ARG=""
if [ "$NO_CACHE" = "true" ]; then
    NO_CACHE_ARG="--no-cache"
fi
PULL_ARG=""
if [ "$PULL" = "true" ]; then
    PULL_ARG="--pull"
fi

print_summary

if [ -n "$BUILD_TOOL" ]; then
    case "$BUILD_TOOL" in
        podman) ;;
        docker|docker-buildx|buildx) BUILD_TOOL="docker-buildx" ;;
        *)
            echo "Error: unsupported build tool '${BUILD_TOOL}'. Expected podman or docker-buildx." >&2
            exit 1
            ;;
    esac
fi

if { [ -z "$BUILD_TOOL" ] || [ "$BUILD_TOOL" = "podman" ]; } && command -v podman >/dev/null 2>&1; then
    echo "Using Podman for build and manifest assembly."
    for workload in "${WORKLOADS[@]}"; do
        build_workload_with_podman "$workload"
        assemble_podman_manifest "$workload"
    done

    if [ "$PUSH" = "true" ]; then
        for workload in "${WORKLOADS[@]}"; do
            push_workload_with_podman "$workload"
        done
    else
        for workload in "${WORKLOADS[@]}"; do
            role_tag="$(role_tag_for "$workload")"
            arch_tag="$(arch_tag_for "$workload")"
            echo "To push ${workload}:"
            echo "  podman push ${IMAGE_NAME}:${arch_tag} docker://${IMAGE_NAME}:${arch_tag}"
            echo "  podman manifest push ${IMAGE_NAME}:${role_tag} docker://${IMAGE_NAME}:${role_tag}"
        done
    fi
elif { [ -z "$BUILD_TOOL" ] || [ "$BUILD_TOOL" = "docker-buildx" ]; } && docker buildx version >/dev/null 2>&1; then
    echo "Using Docker Buildx for native workload builds."
    for workload in "${WORKLOADS[@]}"; do
        build_workload_with_buildx "$workload"
    done
    if [ "$PUSH" != "true" ]; then
        echo "Docker Buildx native builds completed. Manifest assembly from pre-existing platform images is implemented for Podman only."
    fi
elif [ "$BUILD_TOOL" = "podman" ]; then
    echo "Error: podman requested via BUILD_TOOL but podman is not available." >&2
    exit 1
elif [ "$BUILD_TOOL" = "docker-buildx" ]; then
    echo "Error: docker buildx requested via BUILD_TOOL but docker buildx is not available." >&2
    exit 1
else
    echo "Error: neither podman nor docker buildx found." >&2
    exit 1
fi
