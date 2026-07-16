#!/usr/bin/env bash

aarnn_apt_log() {
  printf '[apt] %s\n' "$*" >&2
}

aarnn_apt_build_options() {
  local -n _opts="$1"
  local force_ipv4="${AARNN_APT_FORCE_IPV4:-true}"
  local fetch_retries="${AARNN_APT_FETCH_RETRIES:-5}"
  local http_timeout="${AARNN_APT_HTTP_TIMEOUT_SEC:-30}"
  local pipeline_depth="${AARNN_APT_HTTP_PIPELINE_DEPTH:-0}"
  local queue_mode="${AARNN_APT_QUEUE_MODE:-access}"

  _opts=(
    -o "Acquire::ForceIPv4=${force_ipv4}"
    -o "Acquire::Retries=${fetch_retries}"
    -o "Acquire::http::Timeout=${http_timeout}"
    -o "Acquire::https::Timeout=${http_timeout}"
    -o "Acquire::http::Pipeline-Depth=${pipeline_depth}"
    -o "Acquire::Queue-Mode=${queue_mode}"
  )
}

aarnn_apt_verify_binaries() {
  local required="${AARNN_APT_REQUIRED_BINS:-}"
  local missing=()
  local bin

  [[ -n "$required" ]] || return 0

  for bin in $required; do
    if ! command -v "$bin" >/dev/null 2>&1; then
      missing+=("$bin")
    fi
  done

  if ((${#missing[@]} > 0)); then
    aarnn_apt_log "missing required binaries after install: ${missing[*]}"
    return 1
  fi
}

aarnn_apt_source_hosts() {
  local file uri_line uri host

  while IFS= read -r -d '' file; do
    while IFS= read -r uri_line; do
      for uri in $uri_line; do
        case "$uri" in
          http://*|https://*)
            host="${uri#*://}"
            host="${host%%/*}"
            [[ -n "$host" ]] && printf '%s\n' "$host"
            ;;
        esac
      done
    done < <(
      if [[ "$file" == *.list ]]; then
        awk '
          $1 == "deb" || $1 == "deb-src" {
            if ($2 ~ /^\[/) {
              print $3
            } else {
              print $2
            }
          }
        ' "$file"
      else
        sed -n 's/^[[:space:]]*URIs:[[:space:]]*//p' "$file"
      fi
    )
  done < <(find /etc/apt -maxdepth 2 -type f \( -name '*.list' -o -name '*.sources' \) -print0 2>/dev/null)
}

aarnn_apt_wait_for_dns() {
  local dns_attempts="${AARNN_APT_DNS_ATTEMPTS:-6}"
  local dns_delay="${AARNN_APT_DNS_DELAY_SEC:-2}"
  local attempt host
  local -a hosts=()

  command -v getent >/dev/null 2>&1 || return 0

  mapfile -t hosts < <(aarnn_apt_source_hosts | sort -u)
  ((${#hosts[@]} > 0)) || return 0

  for host in "${hosts[@]}"; do
    attempt=1
    while ! getent ahostsv4 "$host" >/dev/null 2>&1 && ! getent hosts "$host" >/dev/null 2>&1; do
      if ((attempt >= dns_attempts)); then
        aarnn_apt_log "failed to resolve '${host}' after ${dns_attempts} attempts"
        return 1
      fi
      aarnn_apt_log "waiting for DNS for '${host}' (attempt ${attempt}/${dns_attempts})"
      sleep "$dns_delay"
      attempt=$((attempt + 1))
    done
  done
}

aarnn_apt_update_with_retry() {
  local update_attempts="${AARNN_APT_UPDATE_ATTEMPTS:-5}"
  local retry_delay="${AARNN_APT_RETRY_DELAY_SEC:-5}"
  local attempt status sleep_for
  local -a apt_options=()

  aarnn_apt_build_options apt_options

  attempt=1
  while true; do
    if aarnn_apt_wait_for_dns && \
      apt-get "${apt_options[@]}" update; then
      return 0
    fi

    status=$?
    if ((attempt >= update_attempts)); then
      return "$status"
    fi

    sleep_for=$((retry_delay * attempt))
    aarnn_apt_log "apt-get update failed (attempt ${attempt}/${update_attempts}); retrying in ${sleep_for}s"
    sleep "$sleep_for"
    attempt=$((attempt + 1))
  done
}

aarnn_apt_install_with_retry() {
  local install_attempts="${AARNN_APT_INSTALL_ATTEMPTS:-3}"
  local retry_delay="${AARNN_APT_RETRY_DELAY_SEC:-5}"
  local attempt status sleep_for
  local -a apt_options=()

  aarnn_apt_build_options apt_options

  attempt=1
  while true; do
    if aarnn_apt_update_with_retry && \
      apt-get "${apt_options[@]}" install "$@"; then
      if aarnn_apt_verify_binaries; then
        return 0
      fi
    fi

    status=$?
    if ((attempt >= install_attempts)); then
      return "$status"
    fi

    sleep_for=$((retry_delay * attempt))
    aarnn_apt_log "apt-get install failed (attempt ${attempt}/${install_attempts}); retrying in ${sleep_for}s"
    sleep "$sleep_for"
    attempt=$((attempt + 1))
  done
}
