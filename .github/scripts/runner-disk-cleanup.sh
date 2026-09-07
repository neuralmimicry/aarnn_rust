#!/usr/bin/env bash

set -euo pipefail

mode="${1:-report}"
min_free_gib="${AARNN_MIN_FREE_GIB:-20}"
workspace="${GITHUB_WORKSPACE:-}"
runner_temp="${RUNNER_TEMP:-}"

if [[ -z "${workspace}" || -z "${runner_temp}" ]]; then
  echo "GITHUB_WORKSPACE and RUNNER_TEMP are required" >&2
  exit 1
fi

free_kib() {
  df -Pk "$1" | awk 'NR == 2 { print $4 }'
}

report_disks() {
  local path free used percent mount
  local -A seen=()

  for path in / "$runner_temp" "$workspace"; do
    [[ -e "$path" ]] || continue
    mount="$(df -P "$path" | awk 'NR == 2 { print $6 }')"
    [[ -n "${seen[$mount]:-}" ]] && continue
    seen["$mount"]=1
    read -r _ _ used _ percent mount < <(df -hP "$path" | awk 'NR == 2')
    free="$(free_kib "$path")"
    printf 'runner disk: path=%s mount=%s used=%s percent=%s free_gib=%s\n' \
      "$path" "$mount" "$used" "$percent" "$((free / 1024 / 1024))"
  done
}

find_work_root() {
  local probe="$workspace"
  while [[ "$probe" != "/" ]]; do
    if [[ "$(basename "$probe")" == "_work" ]]; then
      printf '%s\n' "$probe"
      return 0
    fi
    probe="$(dirname "$probe")"
  done
  return 1
}

cleanup_stale_workspaces() {
  local work_root current_relative current_job_root candidate candidate_name
  work_root="$(find_work_root || true)"
  [[ -n "$work_root" && -d "$work_root" ]] || return 0

  current_relative="${workspace#"$work_root"/}"
  current_job_root="${current_relative%%/*}"

  while IFS= read -r -d '' candidate; do
    candidate_name="${candidate#"$work_root"/}"
    [[ "$candidate_name" == "$current_job_root" ]] && continue
    if find "$candidate" -mindepth 0 -maxdepth 0 -mmin +120 -print -quit | grep -q .; then
      echo "Removing stale runner workspace: $candidate"
      rm -rf -- "$candidate"
    fi
  done < <(find "$work_root" -mindepth 1 -maxdepth 1 -type d -print0)
}

cleanup_stale_temp() {
  [[ -d "$runner_temp" ]] || return 0
  while IFS= read -r -d '' candidate; do
    if [[ "$candidate" == *"${GITHUB_RUN_ID:-}"* ]]; then
      continue
    fi
    echo "Removing stale runner temporary directory: $candidate"
    rm -rf -- "$candidate"
  done < <(find "$runner_temp" -mindepth 1 -maxdepth 1 -type d -mmin +180 -print0)
}

cleanup_stale_buildah() {
  # Buildah creates temporary layer trees outside the runner workspace.  A
  # killed Podman build can leave several GiB there, so clean only old trees
  # and only while no build tool is running on this serialized runner.
  command -v sudo >/dev/null 2>&1 || return 0
  command -v find >/dev/null 2>&1 || return 0
  if ps -eo comm= | awk '$1 == "buildah" || $1 == "podman" || $1 == "cargo" || $1 == "rustc" { found=1 } END { exit found ? 0 : 1 }'; then
    echo "Active build process detected; retaining stale Buildah directories"
    return 0
  fi
  while IFS= read -r -d '' candidate; do
    echo "Removing stale Buildah temporary directory: $candidate"
    sudo -n rm -rf -- "$candidate" || true
  done < <(find /var/tmp -mindepth 1 -maxdepth 1 -type d -name 'buildah*' -mmin +180 -print0 2>/dev/null)
}

cleanup_system_caches() {
  command -v sudo >/dev/null 2>&1 || return 0
  sudo -n apt-get clean >/dev/null 2>&1 || true
}

remove_repo_build_outputs() {
  local repo="$workspace/repo-build-release"
  [[ -d "$repo" ]] || return 0
  rm -rf -- \
    "$repo/target" \
    "$repo/.container-cache/debs-ci" \
    "$repo/dist/container"
}

remove_job_temp_outputs() {
  local variable value
  for variable in PACKAGE_OUTPUT_DIR CARGO_TARGET_DIR CONTAINER_CARGO_HOME CONTAINER_RUSTUP_HOME \
    AARNN_PODMAN_ROOT AARNN_PODMAN_RUNROOT AARNN_PODMAN_TMPDIR; do
    value="${!variable:-}"
    [[ -n "$value" ]] || continue
    case "$value" in
      "$runner_temp"/*|/tmp/aarnn-p*) rm -rf -- "$value" ;;
    esac
  done

  find "$runner_temp" -mindepth 1 -maxdepth 1 -type d \
    \( -name "podman-runtime-${GITHUB_RUN_ID:-}-*" \
    -o -name "podman-storage-${GITHUB_RUN_ID:-}-*" \
    -o -name "podman-bin" \
    -o -name "cargo-target-*" \
    -o -name "cargo-home-*" \
    -o -name "rustup-home-*" \
    -o -name "release-assets-*" \) \
    -exec rm -rf -- {} +
}

prune_job_podman() {
  command -v podman >/dev/null 2>&1 || return 0
  [[ -n "${CONTAINERS_STORAGE_CONF:-}" ]] || return 0

  local graphroot
  graphroot="$(podman info --format '{{.Store.GraphRoot}}' 2>/dev/null || true)"
  case "$graphroot" in
    "$runner_temp"/*)
      echo "Pruning isolated rootless Podman storage: $graphroot"
      timeout 120 podman system prune --all --force --volumes || true
      ;;
  esac
}

enforce_free_space() {
  local path free required
  required=$((min_free_gib * 1024 * 1024))
  for path in / "$runner_temp" "$workspace"; do
    [[ -e "$path" ]] || continue
    free="$(free_kib "$path")"
    if (( free < required )); then
      echo "::error::Runner mount for $path has only $((free / 1024 / 1024)) GiB free; refusing a disk-heavy build (minimum ${min_free_gib} GiB)." >&2
      return 1
    fi
  done
}

case "$mode" in
  prepare)
    cleanup_stale_workspaces
    cleanup_stale_temp
    cleanup_stale_buildah
    cleanup_system_caches
    remove_repo_build_outputs
    report_disks
    enforce_free_space
    ;;
  prepare-container)
    cleanup_stale_workspaces
    cleanup_stale_temp
    cleanup_stale_buildah
    cleanup_system_caches
    remove_repo_build_outputs
    remove_job_temp_outputs
    prune_job_podman
    report_disks
    enforce_free_space
    ;;
  finish)
    prune_job_podman
    cleanup_stale_buildah
    cleanup_system_caches
    remove_repo_build_outputs
    remove_job_temp_outputs
    report_disks
    ;;
  report)
    report_disks
    ;;
  *)
    echo "usage: $0 {prepare|prepare-container|finish|report}" >&2
    exit 2
    ;;
esac
