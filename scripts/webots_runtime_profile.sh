#!/usr/bin/env bash

# Shared Webots build/runtime profile.  Keep feature selection and the local
# management transport in one place: an explicit Webots binary does not link
# management_v1 and must therefore run with plaintext legacy gRPC.

webots_runtime_profile() {
    local requested="${NM_WEBOTS_RUNTIME_FEATURES:-engine_runtime,ui,robot_io,cuda}"
    case "${requested,,}" in
        all|all-features)
            printf '%s\n' all-features
            ;;
        '')
            printf '%s\n' engine_runtime,ui,robot_io,cuda
            ;;
        *)
            printf '%s\n' "$requested"
            ;;
    esac
}

webots_profile_has_feature() {
    local profile="$1"
    local wanted="$2"
    [ "$profile" = all-features ] && return 0
    local feature
    IFS=',' read -r -a features <<< "$profile"
    for feature in "${features[@]}"; do
        [ "$feature" = "$wanted" ] && return 0
    done
    return 1
}

webots_cargo_profile_args() {
    local profile="$1"
    if [ "$profile" = all-features ]; then
        printf '%s\n' --all-features
    else
        # Callers read this through `read -a`, which consumes one line. Keep
        # the complete argv fragment on that line so the explicit profile is
        # actually passed to Cargo.
        printf '%s\n' "--no-default-features --features $profile"
    fi
}

webots_prepare_management_env() {
    local profile="$1"
    local runtime_root="$2"
    if webots_profile_has_feature "$profile" management_v1; then
        if ! command -v python3 >/dev/null 2>&1; then
            echo "python3 is required to prepare the local management environment" >&2
            return 1
        fi
        eval "$(python3 "$ROOT_DIR/scripts/local_management_env.py" \
            --runtime-root "$runtime_root" --shell)"
    else
        # Do not let credentials from a surrounding all-feature launch turn an
        # explicit plaintext Webots server into a TLS client by accident.
        unset NM_GRPC_TLS_CERT NM_GRPC_TLS_KEY NM_GRPC_TLS_CA NM_GRPC_TLS_DOMAIN
        unset NM_MANAGEMENT_BEARER_TOKEN NM_MANAGEMENT_PRINCIPAL
        unset NM_MANAGEMENT_PRINCIPALS NM_MANAGEMENT_STATE_PATH
    fi
}
