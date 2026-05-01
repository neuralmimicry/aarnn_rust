#!/usr/bin/env bash

# Shared workload metadata for role-specific container builds.

aarnn_container_workload_names() {
    printf '%s\n' standalone orchestrator node web-ui desktop-ui
}

aarnn_container_validate_workload() {
    local workload="$1"
    case "$workload" in
        standalone|orchestrator|node|web-ui|desktop-ui) ;;
        *)
            echo "Unsupported container workload: $workload" >&2
            return 1
            ;;
    esac
}

aarnn_container_workload_features() {
    local workload="$1"
    aarnn_container_validate_workload "$workload" >/dev/null
    case "$workload" in
        standalone) printf '%s' 'standalone_workload' ;;
        orchestrator) printf '%s' 'orchestrator_workload' ;;
        node) printf '%s' 'node_workload' ;;
        web-ui) printf '%s' 'web_ui_workload' ;;
        desktop-ui) printf '%s' 'desktop_ui_workload' ;;
    esac
}

aarnn_container_workload_targets() {
    local workload="$1"
    aarnn_container_validate_workload "$workload" >/dev/null
    case "$workload" in
        web-ui) printf '%s' 'web_ui' ;;
        standalone|orchestrator|node|desktop-ui) printf '%s' 'aarnn_rust' ;;
    esac
}

aarnn_container_workload_tag() {
    local base_tag="$1"
    local workload="$2"
    aarnn_container_validate_workload "$workload" >/dev/null
    printf '%s-%s' "$base_tag" "$workload"
}

aarnn_container_workload_needs_native_ui() {
    local workload="$1"
    aarnn_container_validate_workload "$workload" >/dev/null
    case "$workload" in
        desktop-ui) printf '%s' '1' ;;
        *) printf '%s' '0' ;;
    esac
}

aarnn_container_workload_description() {
    local workload="$1"
    aarnn_container_validate_workload "$workload" >/dev/null
    case "$workload" in
        standalone) printf '%s' 'Single-process continuous engine runtime' ;;
        orchestrator) printf '%s' 'Distributed control-plane engine runtime' ;;
        node) printf '%s' 'Distributed worker engine runtime' ;;
        web-ui) printf '%s' 'HTTP web_ui server with embedded runtime workspace support' ;;
        desktop-ui) printf '%s' 'Native Rust UI and robot IPC runtime' ;;
    esac
}
