#!/usr/bin/env bash

# Shared helpers for local Podman workload test scripts.

aarnn_require_cmd() {
    local cmd="$1"
    command -v "$cmd" >/dev/null 2>&1 || {
        echo "Required command not found: $cmd" >&2
        exit 1
    }
}

aarnn_detect_container_arch() {
    case "$(uname -m)" in
        x86_64|amd64) printf '%s' 'amd64' ;;
        aarch64|arm64) printf '%s' 'arm64' ;;
        *)
            echo "Unsupported host architecture: $(uname -m)" >&2
            exit 1
            ;;
    esac
}

aarnn_default_workload_image() {
    local workload="$1"
    local image_repo="${2:-ghcr.io/neuralmimicry/aarnn_rust}"
    local arch
    arch="$(aarnn_detect_container_arch)"
    printf '%s:%s-%s-%s' "$image_repo" 'engine' "$workload" "$arch"
}

aarnn_append_optional_file_mount() {
    local mounts_name="$1"
    local args_name="$2"
    local host_path="$3"
    local container_path="$4"
    local flag_name="$5"

    if [ -z "$host_path" ] || [ ! -f "$host_path" ]; then
        return 0
    fi

    local -n mounts_ref="$mounts_name"
    local -n args_ref="$args_name"
    mounts_ref+=( -v "$host_path:$container_path:ro,Z" )
    args_ref+=( "$flag_name" "$container_path" )
}

aarnn_find_free_port() {
    local start="${1:-50051}"
    local port="$start"
    while [ "$port" -le 65535 ]; do
        if ! ss -H -ltn | awk '{print $4}' | awk -F: '{print $NF}' | grep -qx "$port" \
            && ! ss -H -lun | awk '{print $4}' | awk -F: '{print $NF}' | grep -qx "$port"; then
            printf '%s' "$port"
            return 0
        fi
        port=$((port + 1))
    done
    echo "No free port found at or above $start" >&2
    return 1
}
