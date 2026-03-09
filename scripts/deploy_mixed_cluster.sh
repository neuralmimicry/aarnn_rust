#!/usr/bin/env bash
# shellcheck disable=SC2016
#
# Deploy or tear down a mixed-architecture k3s cluster.
#
# Topology:
#   - k3s server (control plane) runs on localhost
#   - k3s agents run on remote worker hosts over SSH
#   - application workloads are deployed into the selected namespace
#
# This script intentionally favours:
#   - strict error handling
#   - idempotent operations where practical
#   - verbose, actionable diagnostics on failure
#   - explicit validation of user input
#   - careful cleanup for both deploy and delete workflows

set -Eeuo pipefail

readonly SCRIPT_NAME="$(basename "${BASH_SOURCE[0]}")"
readonly LOG_PREFIX="[deploy-mixed-cluster]"

# ------------------------------------------------------------------------------
# Default configuration
# ------------------------------------------------------------------------------

# Worker hosts are stored in an array to preserve host boundaries safely.
WORKER_HOSTS=("192.168.1.60" "192.168.1.72")

SSH_USER="${SSH_USER:-${USER}}"
SSH_PORT="${SSH_PORT:-22}"
INSTALL_K3S_CHANNEL="${INSTALL_K3S_CHANNEL:-stable}"
ORCHESTRATOR_IMAGE="${ORCHESTRATOR_IMAGE:-ghcr.io/neuralmimicry/aarnn_rust:main}"
NAMESPACE="${NAMESPACE:-default}"
WAIT_TIMEOUT_SECONDS="${WAIT_TIMEOUT_SECONDS:-300}"
INTERACTIVE_SUDO="${INTERACTIVE_SUDO:-true}"
WEB_UI_LOCAL_PORT="${WEB_UI_LOCAL_PORT:-18080}"
REMOTE_INSTALL_TIMEOUT_SECONDS="${REMOTE_INSTALL_TIMEOUT_SECONDS:-900}"
AARNN_NODE_WAIT_SECONDS="${AARNN_NODE_WAIT_SECONDS:-180}"
STOP_SYSTEM_KUBELET="${STOP_SYSTEM_KUBELET:-true}"
STOP_REMOTE_SYSTEM_KUBELET="${STOP_REMOTE_SYSTEM_KUBELET:-true}"
GHCR_USERNAME="${GHCR_USERNAME:-}"
GHCR_TOKEN="${GHCR_TOKEN:-}"
GHCR_EMAIL="${GHCR_EMAIL:-noreply@localhost}"
GHCR_PULL_SECRET_NAME="${GHCR_PULL_SECRET_NAME:-ghcr-pull-secret}"
ROLLOUT_TIMEOUT_SECONDS="${ROLLOUT_TIMEOUT_SECONDS:-600}"
K3S_KUBELET_CPU_FLAGS="${K3S_KUBELET_CPU_FLAGS:---kubelet-arg=cpu-cfs-quota=false --kubelet-arg=cpu-manager-policy=none}"
ENABLE_GPU_PASSTHROUGH="${ENABLE_GPU_PASSTHROUGH:-true}"

# When ACTION=deploy the script installs/updates the cluster.
# When ACTION=delete the script tears down workloads and k3s components.
ACTION="deploy"

# Global SSH options are refreshed after argument parsing because SSH_PORT can
# change at runtime.
SSH_OPTS=()

# A small amount of process-global state is used for cleanup and diagnostics.
PORT_FORWARD_PID=""
PORT_FORWARD_LOG=""
STATUS_LOG=""
KUBECONFIG_CONFIGURED="false"

# ------------------------------------------------------------------------------
# Logging, diagnostics and generic helpers
# ------------------------------------------------------------------------------

log() {
  printf '%s %s\n' "${LOG_PREFIX}" "$*"
}

warn() {
  printf '%s WARNING: %s\n' "${LOG_PREFIX}" "$*" >&2
}

fail() {
  printf '%s ERROR: %s\n' "${LOG_PREFIX}" "$*" >&2
  exit 1
}

on_error() {
  local exit_code="$1"
  local line_no="$2"
  printf '%s ERROR at line %s (exit=%s)\n' "${LOG_PREFIX}" "${line_no}" "${exit_code}" >&2
}

cleanup() {
  # Cleanup is intentionally best-effort. Failures here must not hide the
  # original error that triggered EXIT.
  if [[ -n "${PORT_FORWARD_PID}" ]]; then
    kill "${PORT_FORWARD_PID}" >/dev/null 2>&1 || true
    wait "${PORT_FORWARD_PID}" 2>/dev/null || true
  fi

  [[ -n "${PORT_FORWARD_LOG}" ]] && rm -f "${PORT_FORWARD_LOG}" || true
  [[ -n "${STATUS_LOG}" ]] && rm -f "${STATUS_LOG}" || true
}

trap 'on_error "$?" "$LINENO"' ERR
trap cleanup EXIT

usage() {
  cat <<'USAGE'
Usage: scripts/deploy_mixed_cluster.sh [options]

Actions:
  --delete                    Tear down the running mixed k3s cluster instead of deploying it.

Options:
  --workers <host1,host2>     Comma-separated worker hosts/IPs.
  --ssh-user <user>           SSH user for worker hosts (default: current user).
  --ssh-port <port>           SSH port for worker hosts (default: 22).
  --image <image:tag>         Orchestrator image (default: ghcr.io/neuralmimicry/aarnn_rust:main).
  --namespace <ns>            Kubernetes namespace for application workloads (default: default).
  --k3s-channel <channel>     k3s release channel (default: stable).
  --wait-timeout <seconds>    Wait timeout for node readiness (default: 300).
  --no-interactive-sudo       Require passwordless sudo on worker hosts.
  --web-ui-local-port <port>  Local port for web-ui operational probe (default: 18080).
  --remote-timeout <seconds>  Timeout for each remote k3s agent install/remove (default: 900).
  --node-wait <seconds>       Timeout waiting for AARNN node registration (default: 180).
  --keep-system-kubelet       Do not stop existing kubelet.service on localhost.
  --keep-remote-system-kubelet
                              Do not stop existing kubelet.service on remote worker hosts.
  --ghcr-username <username>  GHCR username for private image pulls.
  --ghcr-token <token>        GHCR token/PAT for private image pulls.
  --ghcr-email <email>        Email for image pull secret (default: noreply@localhost).
  --ghcr-secret <name>        Kubernetes secret name for GHCR pull creds (default: ghcr-pull-secret).
  --rollout-timeout <seconds> Rollout timeout for orchestrator/web-ui/node workloads (default: 600).
  --k3s-kubelet-cpu-flags <flags>
                              Extra kubelet CPU flags for k3s install/update
                              (default: --kubelet-arg=cpu-cfs-quota=false --kubelet-arg=cpu-manager-policy=none).
  --no-gpu-passthrough        Disable default host GPU device passthrough (/dev + /sys) into pods.
  --help                      Show this help.

Environment overrides are supported for the same fields:
  SSH_USER, SSH_PORT, ORCHESTRATOR_IMAGE, NAMESPACE, INSTALL_K3S_CHANNEL,
  WAIT_TIMEOUT_SECONDS, INTERACTIVE_SUDO, WEB_UI_LOCAL_PORT,
  REMOTE_INSTALL_TIMEOUT_SECONDS, AARNN_NODE_WAIT_SECONDS,
  STOP_SYSTEM_KUBELET, STOP_REMOTE_SYSTEM_KUBELET, GHCR_USERNAME, GHCR_TOKEN,
  GHCR_EMAIL, GHCR_PULL_SECRET_NAME, ROLLOUT_TIMEOUT_SECONDS,
  K3S_KUBELET_CPU_FLAGS, ENABLE_GPU_PASSTHROUGH
USAGE
}

require_cmd() {
  local cmd="$1"
  command -v "${cmd}" >/dev/null 2>&1 || fail "Required command not found: ${cmd}"
}

refresh_ssh_opts() {
  SSH_OPTS=(
    -p "${SSH_PORT}"
    -o BatchMode=yes
    -o ConnectTimeout=10
    -o StrictHostKeyChecking=accept-new
  )
}

is_positive_integer() {
  [[ "$1" =~ ^[0-9]+$ ]] && (( "$1" > 0 ))
}

validate_bool() {
  case "$1" in
    true|false) return 0 ;;
    *) return 1 ;;
  esac
}

ensure_positive_integer() {
  local value="$1"
  local field_name="$2"
  is_positive_integer "${value}" || fail "${field_name} must be a positive integer (got: ${value})"
}

normalize_arch() {
  local raw="$1"
  case "${raw}" in
    x86_64|amd64) echo "amd64" ;;
    aarch64|arm64) echo "arm64" ;;
    *) echo "unknown(${raw})" ;;
  esac
}

join_by() {
  local sep="$1"
  shift || true
  local first="true"
  local item
  for item in "$@"; do
    if [[ "${first}" == "true" ]]; then
      printf '%s' "${item}"
      first="false"
    else
      printf '%s%s' "${sep}" "${item}"
    fi
  done
}

parse_workers_csv() {
  local csv="$1"
  local -a parsed=()
  local host

  IFS=',' read -r -a parsed <<<"${csv}"
  ((${#parsed[@]} > 0)) || fail "--workers requires at least one host"

  for host in "${parsed[@]}"; do
    [[ -n "${host}" ]] || fail "--workers contains an empty host entry"
  done

  WORKER_HOSTS=("${parsed[@]}")
}

parse_args() {
  while (($# > 0)); do
    case "$1" in
      --delete)
        ACTION="delete"
        ;;
      --workers)
        shift
        [[ $# -gt 0 ]] || fail "--workers requires a value"
        parse_workers_csv "$1"
        ;;
      --ssh-user)
        shift
        [[ $# -gt 0 ]] || fail "--ssh-user requires a value"
        SSH_USER="$1"
        ;;
      --ssh-port)
        shift
        [[ $# -gt 0 ]] || fail "--ssh-port requires a value"
        SSH_PORT="$1"
        ;;
      --image)
        shift
        [[ $# -gt 0 ]] || fail "--image requires a value"
        ORCHESTRATOR_IMAGE="$1"
        ;;
      --namespace)
        shift
        [[ $# -gt 0 ]] || fail "--namespace requires a value"
        NAMESPACE="$1"
        ;;
      --k3s-channel)
        shift
        [[ $# -gt 0 ]] || fail "--k3s-channel requires a value"
        INSTALL_K3S_CHANNEL="$1"
        ;;
      --wait-timeout)
        shift
        [[ $# -gt 0 ]] || fail "--wait-timeout requires a value"
        WAIT_TIMEOUT_SECONDS="$1"
        ;;
      --no-interactive-sudo)
        INTERACTIVE_SUDO="false"
        ;;
      --web-ui-local-port)
        shift
        [[ $# -gt 0 ]] || fail "--web-ui-local-port requires a value"
        WEB_UI_LOCAL_PORT="$1"
        ;;
      --remote-timeout)
        shift
        [[ $# -gt 0 ]] || fail "--remote-timeout requires a value"
        REMOTE_INSTALL_TIMEOUT_SECONDS="$1"
        ;;
      --node-wait)
        shift
        [[ $# -gt 0 ]] || fail "--node-wait requires a value"
        AARNN_NODE_WAIT_SECONDS="$1"
        ;;
      --keep-system-kubelet)
        STOP_SYSTEM_KUBELET="false"
        ;;
      --keep-remote-system-kubelet)
        STOP_REMOTE_SYSTEM_KUBELET="false"
        ;;
      --ghcr-username)
        shift
        [[ $# -gt 0 ]] || fail "--ghcr-username requires a value"
        GHCR_USERNAME="$1"
        ;;
      --ghcr-token)
        shift
        [[ $# -gt 0 ]] || fail "--ghcr-token requires a value"
        GHCR_TOKEN="$1"
        ;;
      --ghcr-email)
        shift
        [[ $# -gt 0 ]] || fail "--ghcr-email requires a value"
        GHCR_EMAIL="$1"
        ;;
      --ghcr-secret)
        shift
        [[ $# -gt 0 ]] || fail "--ghcr-secret requires a value"
        GHCR_PULL_SECRET_NAME="$1"
        ;;
      --rollout-timeout)
        shift
        [[ $# -gt 0 ]] || fail "--rollout-timeout requires a value"
        ROLLOUT_TIMEOUT_SECONDS="$1"
        ;;
      --k3s-kubelet-cpu-flags)
        shift
        [[ $# -gt 0 ]] || fail "--k3s-kubelet-cpu-flags requires a value"
        K3S_KUBELET_CPU_FLAGS="$1"
        ;;
      --no-gpu-passthrough)
        ENABLE_GPU_PASSTHROUGH="false"
        ;;
      --help|-h)
        usage
        exit 0
        ;;
      *)
        fail "Unknown argument: $1"
        ;;
    esac
    shift
  done
}

validate_config() {
  ensure_positive_integer "${SSH_PORT}" "SSH port"
  ensure_positive_integer "${WAIT_TIMEOUT_SECONDS}" "Wait timeout"
  ensure_positive_integer "${WEB_UI_LOCAL_PORT}" "Web UI local port"
  ensure_positive_integer "${REMOTE_INSTALL_TIMEOUT_SECONDS}" "Remote install timeout"
  ensure_positive_integer "${AARNN_NODE_WAIT_SECONDS}" "Node wait timeout"
  ensure_positive_integer "${ROLLOUT_TIMEOUT_SECONDS}" "Rollout timeout"

  validate_bool "${INTERACTIVE_SUDO}" || fail "INTERACTIVE_SUDO must be true or false"
  validate_bool "${STOP_SYSTEM_KUBELET}" || fail "STOP_SYSTEM_KUBELET must be true or false"
  validate_bool "${STOP_REMOTE_SYSTEM_KUBELET}" || fail "STOP_REMOTE_SYSTEM_KUBELET must be true or false"
  validate_bool "${ENABLE_GPU_PASSTHROUGH}" || fail "ENABLE_GPU_PASSTHROUGH must be true or false"

  [[ -n "${SSH_USER}" ]] || fail "SSH user must not be empty"
  [[ -n "${NAMESPACE}" ]] || fail "Namespace must not be empty"
  [[ -n "${INSTALL_K3S_CHANNEL}" ]] || fail "k3s channel must not be empty"

  refresh_ssh_opts
}

run_with_optional_timeout() {
  local timeout_seconds="$1"
  shift

  if command -v timeout >/dev/null 2>&1; then
    timeout "${timeout_seconds}" "$@"
  else
    "$@"
  fi
}

remote_sudo_mode() {
  local host="$1"
  if ssh "${SSH_OPTS[@]}" "${SSH_USER}@${host}" "LC_ALL=C LANG=C sudo -n true" >/dev/null 2>&1; then
    echo "nopass"
  else
    echo "password"
  fi
}

prompt_for_remote_sudo_password() {
  local host="$1"
  local password=""
  local attempt

  if [[ "${INTERACTIVE_SUDO}" != "true" ]]; then
    fail "Host ${host} requires a sudo password. Re-run without --no-interactive-sudo or configure passwordless sudo."
  fi

  for attempt in 1 2 3; do
    read -rsp "${LOG_PREFIX} sudo password for ${SSH_USER}@${host}: " password
    echo
    if printf '%s\n' "${password}" | ssh "${SSH_OPTS[@]}" "${SSH_USER}@${host}" "LC_ALL=C LANG=C sudo -S -p '' -k true" >/dev/null 2>&1; then
      printf '%s' "${password}"
      return 0
    fi
    warn "Incorrect sudo password for ${host} (attempt ${attempt}/3)."
  done

  fail "Failed to validate sudo password on ${host} after 3 attempts."
}

run_remote_as_root() {
  local host="$1"
  local script_body="$2"
  local timeout_seconds="${3:-0}"

  local mode=""
  local quoted_script=""
  local remote_cmd=""
  local sudo_password=""

  printf -v quoted_script "%q" "${script_body}"
  mode="$(remote_sudo_mode "${host}")"

  if [[ "${mode}" == "nopass" ]]; then
    printf -v remote_cmd 'LC_ALL=C LANG=C sudo -n bash -lc %s' "${quoted_script}"
    if (( timeout_seconds > 0 )); then
      run_with_optional_timeout "${timeout_seconds}" ssh "${SSH_OPTS[@]}" "${SSH_USER}@${host}" "${remote_cmd}"
    else
      ssh "${SSH_OPTS[@]}" "${SSH_USER}@${host}" "${remote_cmd}"
    fi
    return 0
  fi

  sudo_password="$(prompt_for_remote_sudo_password "${host}")"
  printf -v remote_cmd 'LC_ALL=C LANG=C sudo -S -p "" bash -lc %s' "${quoted_script}"
  if (( timeout_seconds > 0 )); then
    printf '%s\n' "${sudo_password}" | run_with_optional_timeout "${timeout_seconds}" ssh "${SSH_OPTS[@]}" "${SSH_USER}@${host}" "${remote_cmd}"
  else
    printf '%s\n' "${sudo_password}" | ssh "${SSH_OPTS[@]}" "${SSH_USER}@${host}" "${remote_cmd}"
  fi
}

kubectl_available() {
  command -v kubectl >/dev/null 2>&1
}

configure_local_kubeconfig() {
  # k3s writes the canonical kubeconfig as root. We copy it into the user's
  # home directory so subsequent kubectl operations can run unprivileged.
  [[ -f /etc/rancher/k3s/k3s.yaml ]] || fail "Expected kubeconfig at /etc/rancher/k3s/k3s.yaml was not found"

  mkdir -p "${HOME}/.kube"
  sudo cp /etc/rancher/k3s/k3s.yaml "${HOME}/.kube/config"
  sudo chown "${USER}:${USER}" "${HOME}/.kube/config"
  chmod 600 "${HOME}/.kube/config"
  export KUBECONFIG="${HOME}/.kube/config"
  KUBECONFIG_CONFIGURED="true"
}

try_configure_local_kubeconfig() {
  if [[ -f /etc/rancher/k3s/k3s.yaml ]]; then
    configure_local_kubeconfig
    return 0
  fi
  return 1
}

k3s_server_installed_local() {
  [[ -x /usr/local/bin/k3s-uninstall.sh ]] || command -v k3s >/dev/null 2>&1 || systemctl list-unit-files | grep -q '^k3s\.service'
}

k3s_agent_installed_remote() {
  local host="$1"
  ssh "${SSH_OPTS[@]}" "${SSH_USER}@${host}" \
    "test -x /usr/local/bin/k3s-agent-uninstall.sh || test -f /etc/systemd/system/k3s-agent.service || systemctl list-unit-files 2>/dev/null | grep -q '^k3s-agent\.service'" \
    >/dev/null 2>&1
}

get_control_plane_ip() {
  if [[ -n "${CONTROL_PLANE_IP:-}" ]]; then
    printf '%s\n' "${CONTROL_PLANE_IP}"
    return 0
  fi

  ip -4 route get 1.1.1.1 2>/dev/null | awk '
    {
      for (i = 1; i <= NF; i++) {
        if ($i == "src" && (i + 1) <= NF) {
          print $(i + 1)
          exit
        }
      }
    }
  '
}

show_local_k3s_diagnostics() {
  log "Local k3s diagnostics:"
  sudo systemctl status k3s --no-pager -l || true
  sudo journalctl -u k3s -n 120 --no-pager || true
  log "Port owners for 10248/10250:"
  sudo ss -lntp '( sport = :10248 or sport = :10250 )' || true
  if command -v lsof >/dev/null 2>&1; then
    sudo lsof -nP -iTCP:10248 -sTCP:LISTEN || true
    sudo lsof -nP -iTCP:10250 -sTCP:LISTEN || true
  fi
}

recover_local_k3s_conflicts() {
  log "Attempting local k3s recovery for kubelet port conflicts."
  sudo systemctl stop k3s kubelet k3s-agent >/dev/null 2>&1 || true
  if [[ -x /usr/local/bin/k3s-killall.sh ]]; then
    sudo /usr/local/bin/k3s-killall.sh || true
  fi
  sleep 2
  sudo ss -lntp '( sport = :10248 or sport = :10250 )' || true
}

prepare_local_runtime_for_k3s() {
  if sudo systemctl is-active --quiet kubelet; then
    if [[ "${STOP_SYSTEM_KUBELET}" != "true" ]]; then
      fail "kubelet.service is active and conflicts with k3s on ports 10248/10250. Stop it or rerun without --keep-system-kubelet."
    fi
    log "Stopping active kubelet.service to avoid local port conflicts with k3s."
    sudo systemctl stop kubelet || true
  fi
}

wait_for_local_k3s_api() {
  local elapsed=0
  local retried=0

  while true; do
    if sudo systemctl is-active --quiet k3s && kubectl get --raw='/readyz' >/dev/null 2>&1; then
      log "Local k3s API is reachable."
      return 0
    fi

    if (( elapsed >= WAIT_TIMEOUT_SECONDS )); then
      show_local_k3s_diagnostics
      if (( retried == 0 )); then
        retried=1
        elapsed=0
        recover_local_k3s_conflicts
        sudo systemctl restart k3s || true
        continue
      fi
      fail "Local k3s API is not reachable."
    fi

    sleep 2
    elapsed=$((elapsed + 2))
  done
}

detect_remote_arch() {
  local host="$1"
  ssh "${SSH_OPTS[@]}" "${SSH_USER}@${host}" "LC_ALL=C LANG=C uname -m"
}

install_k3s_server_local() {
  local node_name=""
  node_name="$(hostname -s)"

  log "Installing or refreshing k3s server on localhost (${node_name}) via channel '${INSTALL_K3S_CHANNEL}'."
  curl -sfL https://get.k3s.io | sudo INSTALL_K3S_CHANNEL="${INSTALL_K3S_CHANNEL}" \
    INSTALL_K3S_EXEC="server --node-name ${node_name} --write-kubeconfig-mode 644 ${K3S_KUBELET_CPU_FLAGS}" sh -

  sudo systemctl enable k3s >/dev/null 2>&1 || true
  if ! sudo systemctl restart k3s; then
    log "Initial local k3s start failed."
    show_local_k3s_diagnostics
    recover_local_k3s_conflicts
    if ! sudo systemctl restart k3s; then
      show_local_k3s_diagnostics
      fail "Local k3s failed to start after recovery attempt."
    fi
  fi
}

install_k3s_agent_remote() {
  local host="$1"
  local control_plane_ip="$2"
  local node_token="$3"
  local remote_script=""

  log "Installing or refreshing k3s agent on ${host}."

  remote_script="$(cat <<REMOTE
set -Eeuo pipefail

export INSTALL_K3S_CHANNEL=$(printf '%q' "${INSTALL_K3S_CHANNEL}")
export K3S_URL=$(printf '%q' "https://${control_plane_ip}:6443")
export K3S_TOKEN=$(printf '%q' "${node_token}")
export STOP_REMOTE_SYSTEM_KUBELET=$(printf '%q' "${STOP_REMOTE_SYSTEM_KUBELET}")
export K3S_KUBELET_CPU_FLAGS=$(printf '%q' "${K3S_KUBELET_CPU_FLAGS}")

command -v curl >/dev/null 2>&1 || { echo "curl is required on worker host." >&2; exit 1; }

if systemctl is-active --quiet kubelet; then
  if [[ "\${STOP_REMOTE_SYSTEM_KUBELET}" == "true" ]]; then
    systemctl stop kubelet || true
  else
    echo "kubelet.service is active on worker and conflicts with k3s-agent ports 10248/10250." >&2
    exit 1
  fi
fi

systemctl stop k3s-agent >/dev/null 2>&1 || true

if ss -lntp '( sport = :10248 or sport = :10250 )' 2>/dev/null | grep -q LISTEN; then
  if [[ "\${STOP_REMOTE_SYSTEM_KUBELET}" == "true" ]]; then
    systemctl stop kubelet >/dev/null 2>&1 || true
  fi
fi

k3s_exec="agent"
if [[ -n "\${K3S_KUBELET_CPU_FLAGS}" ]]; then
  k3s_exec="\${k3s_exec} \${K3S_KUBELET_CPU_FLAGS}"
fi

curl -sfL https://get.k3s.io | INSTALL_K3S_CHANNEL="\${INSTALL_K3S_CHANNEL}" INSTALL_K3S_EXEC="\${k3s_exec}" sh -
systemctl enable --now k3s-agent || true

for _ in \$(seq 1 30); do
  if systemctl is-active --quiet k3s-agent; then
    exit 0
  fi
  systemctl start k3s-agent || true
  sleep 2
done

echo "k3s-agent did not become active in time." >&2
ss -lntp '( sport = :10248 or sport = :10250 )' || true
systemctl status kubelet --no-pager -l || true
systemctl status k3s-agent --no-pager -l || true
journalctl -u k3s-agent -n 120 --no-pager || true
exit 1
REMOTE
)"

  if ! run_remote_as_root "${host}" "${remote_script}" "${REMOTE_INSTALL_TIMEOUT_SECONDS}"; then
    warn "Failed to install/start k3s-agent on ${host}; collecting diagnostics."
    run_remote_as_root "${host}" \
      "ss -lntp '( sport = :10248 or sport = :10250 )' || true; systemctl status kubelet --no-pager -l || true; systemctl status k3s-agent --no-pager -l || true; journalctl -u k3s-agent -n 120 --no-pager || true" \
      60 || true
    fail "k3s-agent installation failed on ${host}."
  fi

  log "k3s agent ready on ${host}."
}

wait_for_nodes_ready() {
  local expected_count="$1"
  local elapsed=0

  while (( elapsed < WAIT_TIMEOUT_SECONDS )); do
    local nodes_output=""
    local count=0
    local ready=0

    nodes_output="$(kubectl get nodes --no-headers 2>/dev/null || true)"
    count="$(awk 'NF > 0 { c++ } END { print c + 0 }' <<<"${nodes_output}")"
    ready="$(awk '$2 ~ /^Ready/ { c++ } END { print c + 0 }' <<<"${nodes_output}")"

    if (( count >= expected_count && ready >= expected_count )); then
      log "Cluster is ready (${ready}/${count} Ready, expected >= ${expected_count})."
      return 0
    fi

    sleep 5
    elapsed=$((elapsed + 5))
  done

  kubectl get nodes -o wide || true
  fail "Timed out waiting for ${expected_count} Ready nodes."
}

configure_registry_auth() {
  kubectl get namespace "${NAMESPACE}" >/dev/null 2>&1 || kubectl create namespace "${NAMESPACE}"

  if [[ -n "${GHCR_USERNAME}" && -n "${GHCR_TOKEN}" ]]; then
    log "Configuring GHCR image pull secret '${GHCR_PULL_SECRET_NAME}' in namespace '${NAMESPACE}'."
    kubectl -n "${NAMESPACE}" create secret docker-registry "${GHCR_PULL_SECRET_NAME}" \
      --docker-server=ghcr.io \
      --docker-username="${GHCR_USERNAME}" \
      --docker-password="${GHCR_TOKEN}" \
      --docker-email="${GHCR_EMAIL}" \
      --dry-run=client -o yaml | kubectl apply -f -
  elif [[ "${ORCHESTRATOR_IMAGE}" == ghcr.io/* ]]; then
    warn "No GHCR credentials provided. If the image is private, set GHCR_USERNAME and GHCR_TOKEN (or use --ghcr-username/--ghcr-token)."
  fi
}

show_workload_debug() {
  log "Collecting Kubernetes workload diagnostics (namespace=${NAMESPACE})."
  kubectl -n "${NAMESPACE}" get deploy,ds,rs,pods,svc -l app=neuromorphic -o wide || true
  kubectl -n "${NAMESPACE}" get events --sort-by=.lastTimestamp | tail -n 120 || true

  while IFS= read -r pod; do
    [[ -n "${pod}" ]] || continue
    kubectl -n "${NAMESPACE}" describe "${pod}" || true
    kubectl -n "${NAMESPACE}" logs "${pod}" --all-containers=true --tail=120 || true
    kubectl -n "${NAMESPACE}" logs "${pod}" --all-containers=true --previous --tail=120 || true
  done < <(kubectl -n "${NAMESPACE}" get pods -l app=neuromorphic -o name || true)
}

show_node_connectivity_debug() {
  local orchestrator_cluster_ip=""
  local web_ui_cluster_ip=""
  local kube_dns_cluster_ip=""

  orchestrator_cluster_ip="$(kubectl -n "${NAMESPACE}" get svc orchestrator -o jsonpath='{.spec.clusterIP}' 2>/dev/null || true)"
  web_ui_cluster_ip="$(kubectl -n "${NAMESPACE}" get svc web-ui -o jsonpath='{.spec.clusterIP}' 2>/dev/null || true)"
  kube_dns_cluster_ip="$(kubectl -n kube-system get svc kube-dns -o jsonpath='{.spec.clusterIP}' 2>/dev/null || true)"

  log "Collecting AARNN node connectivity diagnostics."
  while IFS=' ' read -r pod_name node_name pod_ip; do
    [[ -n "${pod_name}" ]] || continue
    log "Node pod probe: pod=${pod_name}, node=${node_name}, podIP=${pod_ip}"
    kubectl -n "${NAMESPACE}" exec "${pod_name}" -- \
      env \
        ORCH_CLUSTER_IP="${orchestrator_cluster_ip}" \
        WEBUI_CLUSTER_IP="${web_ui_cluster_ip}" \
        KUBEDNS_CLUSTER_IP="${kube_dns_cluster_ip}" \
      sh -lc 'python3 - <<'"'"'PY'"'"'
import os
import socket


def tcp_check(host: str, port: int) -> str:
    if not host:
        return "skip(empty-host)"
    sock = socket.socket()
    sock.settimeout(3.0)
    try:
        sock.connect((host, port))
        return "ok"
    except Exception as exc:
        return f"fail({exc})"
    finally:
        sock.close()


def dns_check(name: str) -> str:
    try:
        infos = socket.getaddrinfo(name, 50051, socket.AF_UNSPEC, socket.SOCK_STREAM)
        addrs = sorted({item[4][0] for item in infos if item and len(item) >= 5})
        return "ok->" + ",".join(addrs)
    except Exception as exc:
        return f"fail({exc})"


print("dns(orchestrator)=" + dns_check("orchestrator"))
print("tcp(orchestrator:50051)=" + tcp_check("orchestrator", 50051))
print("tcp(ORCH_CLUSTER_IP:50051)=" + tcp_check(os.environ.get("ORCH_CLUSTER_IP", ""), 50051))
print("tcp(WEBUI_CLUSTER_IP:8080)=" + tcp_check(os.environ.get("WEBUI_CLUSTER_IP", ""), 8080))
print("tcp(KUBEDNS_CLUSTER_IP:53)=" + tcp_check(os.environ.get("KUBEDNS_CLUSTER_IP", ""), 53))
PY' || true
    kubectl -n "${NAMESPACE}" logs "${pod_name}" --tail=80 || true
  done < <(kubectl -n "${NAMESPACE}" get pods -l role=node -o jsonpath='{range .items[*]}{.metadata.name}{" "}{.spec.nodeName}{" "}{.status.podIP}{"\n"}{end}' || true)
}

rollout_or_fail() {
  local kind="$1"
  local name="$2"
  local timeout="$3"

  if ! kubectl -n "${NAMESPACE}" rollout status "${kind}/${name}" --timeout="${timeout}"; then
    show_workload_debug
    fail "Rollout failed for ${kind}/${name}."
  fi
}

deploy_orchestrator() {
  local control_plane_node_name=""
  local sa_pull_secret_yaml=""
  local pod_pull_secret_yaml=""
  local image_pull_policy="IfNotPresent"
  local force_rollout_restart="false"
  local gpu_pod_volumes_yaml=""
  local gpu_container_mounts_yaml=""
  local gpu_container_security_yaml=""

  control_plane_node_name="$(hostname -s)"

  if [[ "${ORCHESTRATOR_IMAGE}" != *@sha256:* ]]; then
    image_pull_policy="Always"
    force_rollout_restart="true"
  fi

  if [[ -n "${GHCR_USERNAME}" && -n "${GHCR_TOKEN}" ]]; then
    sa_pull_secret_yaml=$'imagePullSecrets:\n  - name: '"${GHCR_PULL_SECRET_NAME}"
    pod_pull_secret_yaml=$'      imagePullSecrets:\n        - name: '"${GHCR_PULL_SECRET_NAME}"
  fi

  if [[ "${ENABLE_GPU_PASSTHROUGH}" == "true" ]]; then
    gpu_container_security_yaml=$'          securityContext:\n            privileged: true\n            runAsUser: 0\n            runAsGroup: 0\n            allowPrivilegeEscalation: true'
    gpu_container_mounts_yaml=$'          volumeMounts:\n            - name: host-dev\n              mountPath: /dev\n            - name: host-sys\n              mountPath: /sys\n              readOnly: true'
    gpu_pod_volumes_yaml=$'      volumes:\n        - name: host-dev\n          hostPath:\n            path: /dev\n            type: Directory\n        - name: host-sys\n          hostPath:\n            path: /sys\n            type: Directory'
  fi

  log "Deploying orchestrator (${ORCHESTRATOR_IMAGE}) to namespace '${NAMESPACE}', pinned to ${control_plane_node_name}."
  cat <<EOF | kubectl -n "${NAMESPACE}" apply -f -
apiVersion: v1
kind: ServiceAccount
metadata:
  name: neuromorphic
  labels:
    app: neuromorphic
${sa_pull_secret_yaml}
---
apiVersion: v1
kind: Service
metadata:
  name: orchestrator
  labels:
    app: neuromorphic
    role: orchestrator
spec:
  ports:
    - name: grpc
      port: 50051
      targetPort: 50051
    - name: discovery
      port: 50050
      protocol: UDP
      targetPort: 50050
  selector:
    app: neuromorphic
    role: orchestrator
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: orchestrator
  labels:
    app: neuromorphic
    role: orchestrator
spec:
  replicas: 1
  selector:
    matchLabels:
      app: neuromorphic
      role: orchestrator
  template:
    metadata:
      labels:
        app: neuromorphic
        role: orchestrator
    spec:
      serviceAccountName: neuromorphic
${pod_pull_secret_yaml}
      nodeSelector:
        kubernetes.io/hostname: ${control_plane_node_name}
      tolerations:
        - key: node-role.kubernetes.io/control-plane
          operator: Exists
          effect: NoSchedule
      containers:
        - name: neuromorphic
          image: ${ORCHESTRATOR_IMAGE}
          imagePullPolicy: ${image_pull_policy}
${gpu_container_security_yaml}
          command: ["/bin/sh", "-lc"]
          args:
            - |
              CORES="\$(nproc --all 2>/dev/null || nproc || echo 1)"
              export RAYON_NUM_THREADS="\${CORES}"
              export TOKIO_WORKER_THREADS="\${CORES}"
              export NM_GA_RESERVE_CORES=0
              exec ./aarnn_rust --orchestrator --grpc-addr 0.0.0.0:50051
${gpu_container_mounts_yaml}
          ports:
            - containerPort: 50051
            - containerPort: 50050
              protocol: UDP
${gpu_pod_volumes_yaml}
---
apiVersion: v1
kind: Service
metadata:
  name: web-ui
  labels:
    app: neuromorphic
    role: web-ui
spec:
  ports:
    - name: http
      port: 8080
      targetPort: 8080
  selector:
    app: neuromorphic
    role: web-ui
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: web-ui
  labels:
    app: neuromorphic
    role: web-ui
spec:
  replicas: 1
  selector:
    matchLabels:
      app: neuromorphic
      role: web-ui
  template:
    metadata:
      labels:
        app: neuromorphic
        role: web-ui
    spec:
      serviceAccountName: neuromorphic
${pod_pull_secret_yaml}
      nodeSelector:
        kubernetes.io/hostname: ${control_plane_node_name}
      tolerations:
        - key: node-role.kubernetes.io/control-plane
          operator: Exists
          effect: NoSchedule
      containers:
        - name: web-ui
          image: ${ORCHESTRATOR_IMAGE}
          imagePullPolicy: ${image_pull_policy}
${gpu_container_security_yaml}
          command: ["/bin/sh", "-lc"]
          args:
            - |
              CORES="\$(nproc --all 2>/dev/null || nproc || echo 1)"
              export RAYON_NUM_THREADS="\${CORES}"
              export TOKIO_WORKER_THREADS="\${CORES}"
              exec ./web_ui --listen 0.0.0.0:8080 --orchestrator http://orchestrator:50051
${gpu_container_mounts_yaml}
          ports:
            - containerPort: 8080
${gpu_pod_volumes_yaml}
---
apiVersion: apps/v1
kind: DaemonSet
metadata:
  name: aarnn-node
  labels:
    app: neuromorphic
    role: node
spec:
  selector:
    matchLabels:
      app: neuromorphic
      role: node
  template:
    metadata:
      labels:
        app: neuromorphic
        role: node
    spec:
      serviceAccountName: neuromorphic
${pod_pull_secret_yaml}
      affinity:
        nodeAffinity:
          requiredDuringSchedulingIgnoredDuringExecution:
            nodeSelectorTerms:
              - matchExpressions:
                  - key: node-role.kubernetes.io/control-plane
                    operator: DoesNotExist
      containers:
        - name: neuromorphic-node
          image: ${ORCHESTRATOR_IMAGE}
          imagePullPolicy: ${image_pull_policy}
${gpu_container_security_yaml}
          command: ["/bin/sh", "-lc"]
          args:
            - |
              CORES="\$(nproc --all 2>/dev/null || nproc || echo 1)"
              export RAYON_NUM_THREADS="\${CORES}"
              export TOKIO_WORKER_THREADS="\${CORES}"
              export NM_GA_RESERVE_CORES=0
              exec ./aarnn_rust --node --orchestrator-addr http://orchestrator:50051 --brain-id cluster
${gpu_container_mounts_yaml}
${gpu_pod_volumes_yaml}
EOF

  if [[ "${force_rollout_restart}" == "true" ]]; then
    log "Mutable image tag detected; forcing rollout restart to pull latest image layers."
    kubectl -n "${NAMESPACE}" rollout restart deployment/orchestrator >/dev/null
    kubectl -n "${NAMESPACE}" rollout restart deployment/web-ui >/dev/null
    kubectl -n "${NAMESPACE}" rollout restart daemonset/aarnn-node >/dev/null
  fi

  rollout_or_fail deployment orchestrator "${ROLLOUT_TIMEOUT_SECONDS}s"
  rollout_or_fail deployment web-ui "${ROLLOUT_TIMEOUT_SECONDS}s"
  rollout_or_fail daemonset aarnn-node "${ROLLOUT_TIMEOUT_SECONDS}s"
}

verify_aarnn_network_operational() {
  local orchestrator_eps=""
  local webui_eps=""
  local probe_ok=0
  local config_probe_ok=0
  local elapsed=0
  local expected_nodes=0
  local registered_nodes=-1
  local status_preview=""

  require_cmd curl
  require_cmd python3
  expected_nodes="${#WORKER_HOSTS[@]}"

  orchestrator_eps="$(kubectl -n "${NAMESPACE}" get endpoints orchestrator -o jsonpath='{.subsets[*].addresses[*].ip}' 2>/dev/null || true)"
  webui_eps="$(kubectl -n "${NAMESPACE}" get endpoints web-ui -o jsonpath='{.subsets[*].addresses[*].ip}' 2>/dev/null || true)"

  [[ -n "${orchestrator_eps// /}" ]] || fail "Orchestrator service has no endpoints."
  [[ -n "${webui_eps// /}" ]] || fail "Web UI service has no endpoints."

  PORT_FORWARD_LOG="$(mktemp)"
  STATUS_LOG="$(mktemp)"

  kubectl -n "${NAMESPACE}" port-forward svc/web-ui "${WEB_UI_LOCAL_PORT}:8080" >"${PORT_FORWARD_LOG}" 2>&1 &
  PORT_FORWARD_PID=$!

  while (( elapsed < AARNN_NODE_WAIT_SECONDS )); do
    if curl -fsS --max-time 5 "http://127.0.0.1:${WEB_UI_LOCAL_PORT}/api/status" >"${STATUS_LOG}" 2>/dev/null; then
      probe_ok=1
      registered_nodes="$(python3 - "${STATUS_LOG}" <<'PY'
import json
import sys

path = sys.argv[1]
try:
    with open(path, "r", encoding="utf-8") as handle:
        data = json.load(handle)
    nodes = data.get("nodes", [])
    if isinstance(nodes, list):
        print(len(nodes))
    else:
        print(-1)
except Exception:
    print(-1)
PY
)"
      if [[ "${registered_nodes}" =~ ^[0-9]+$ ]] && (( registered_nodes >= expected_nodes )); then
        if curl -fsS --max-time 5 "http://127.0.0.1:${WEB_UI_LOCAL_PORT}/api/config" >/dev/null 2>&1; then
          config_probe_ok=1
        fi
        break
      fi
    fi
    sleep 2
    elapsed=$((elapsed + 2))
  done

  if [[ ${probe_ok} -ne 1 ]]; then
    [[ -n "${PORT_FORWARD_LOG}" ]] && cat "${PORT_FORWARD_LOG}" >&2 || true
    fail "AARNN operational probe failed: GET /api/status from web-ui did not succeed."
  fi

  if [[ ! "${registered_nodes}" =~ ^[0-9]+$ ]] || (( registered_nodes < expected_nodes )); then
    status_preview="$(head -c 320 "${STATUS_LOG}" | tr '\n' ' ')"
    kubectl -n "${NAMESPACE}" get pods -l role=node -o wide || true
    show_node_connectivity_debug
    fail "AARNN node registration incomplete: expected >= ${expected_nodes}, observed=${registered_nodes}. Status sample: ${status_preview}"
  fi

  status_preview="$(head -c 320 "${STATUS_LOG}" | tr '\n' ' ')"
  log "AARNN operational probe succeeded via web-ui (/api/status), registered nodes=${registered_nodes}."
  log "Status sample: ${status_preview}"
  if [[ ${config_probe_ok} -ne 1 ]]; then
    warn "web-ui /api/config endpoint is unavailable in the deployed image; dashboard auto-connect may fail until workloads are updated."
  fi
}

# ------------------------------------------------------------------------------
# Delete / teardown workflow
# ------------------------------------------------------------------------------

delete_workloads_if_present() {
  # Workload deletion is intentionally tolerant of a partially-dead control
  # plane. If kubectl or the API is unavailable we log and continue with the
  # host-level teardown.
  if ! kubectl_available; then
    warn "kubectl is not available locally; skipping workload deletion."
    return 0
  fi

  if [[ "${KUBECONFIG_CONFIGURED}" != "true" ]]; then
    try_configure_local_kubeconfig || {
      warn "Local kubeconfig was not found; skipping workload deletion."
      return 0
    }
  fi

  if ! kubectl get --raw='/readyz' >/dev/null 2>&1; then
    warn "Kubernetes API is not reachable; skipping workload deletion."
    return 0
  fi

  log "Deleting neuromorphic workloads from namespace '${NAMESPACE}' (best-effort)."
  kubectl -n "${NAMESPACE}" delete deployment/orchestrator deployment/web-ui daemonset/aarnn-node --ignore-not-found --wait=false || true
  kubectl -n "${NAMESPACE}" delete service/orchestrator service/web-ui serviceaccount/neuromorphic --ignore-not-found --wait=false || true
  kubectl -n "${NAMESPACE}" delete secret/"${GHCR_PULL_SECRET_NAME}" --ignore-not-found --wait=false || true
}

remove_remote_k3s_agent() {
  local host="$1"
  local remote_script=""

  if ! k3s_agent_installed_remote "${host}"; then
    log "Remote worker ${host} does not appear to have k3s-agent installed; nothing to remove."
    return 0
  fi

  log "Removing k3s agent from ${host}."

  remote_script='
set -Eeuo pipefail

if [[ -x /usr/local/bin/k3s-agent-uninstall.sh ]]; then
  /usr/local/bin/k3s-agent-uninstall.sh
else
  systemctl disable --now k3s-agent >/dev/null 2>&1 || true
  if [[ -x /usr/local/bin/k3s-killall.sh ]]; then
    /usr/local/bin/k3s-killall.sh || true
  fi
  rm -f /etc/systemd/system/k3s-agent.service || true
  systemctl daemon-reload || true
fi
'

  if ! run_remote_as_root "${host}" "${remote_script}" "${REMOTE_INSTALL_TIMEOUT_SECONDS}"; then
    warn "Failed to remove k3s-agent cleanly from ${host}; collecting diagnostics."
    run_remote_as_root "${host}" \
      "systemctl status k3s-agent --no-pager -l || true; journalctl -u k3s-agent -n 120 --no-pager || true" \
      60 || true
    fail "k3s-agent teardown failed on ${host}."
  fi

  log "k3s agent removed from ${host}."
}

remove_local_k3s_server() {
  if ! k3s_server_installed_local; then
    log "Local k3s server does not appear to be installed; nothing to remove."
    return 0
  fi

  log "Removing local k3s server."

  if [[ -x /usr/local/bin/k3s-uninstall.sh ]]; then
    sudo /usr/local/bin/k3s-uninstall.sh
  else
    sudo systemctl disable --now k3s >/dev/null 2>&1 || true
    sudo systemctl disable --now k3s-agent >/dev/null 2>&1 || true
    if [[ -x /usr/local/bin/k3s-killall.sh ]]; then
      sudo /usr/local/bin/k3s-killall.sh || true
    fi
    sudo rm -f /etc/systemd/system/k3s.service /etc/systemd/system/k3s-agent.service || true
    sudo systemctl daemon-reload || true
  fi

  log "Local k3s server removed."
}

delete_cluster() {
  local host=""

  log "Starting mixed-cluster teardown."

  # Workloads are removed first while the API may still be reachable.
  delete_workloads_if_present

  # Remote workers are torn down before the control plane so that uninstall
  # scripts can still talk to systemd cleanly and we keep the shutdown order
  # intuitive.
  for host in "${WORKER_HOSTS[@]}"; do
    [[ -n "${host}" ]] || continue
    remove_remote_k3s_agent "${host}"
  done

  remove_local_k3s_server
  log "Teardown complete."
}

# ------------------------------------------------------------------------------
# Deploy workflow
# ------------------------------------------------------------------------------

deploy_cluster() {
  local local_raw_arch=""
  local local_norm_arch=""
  local control_plane_ip=""
  local node_token=""
  local host=""
  local remote_raw_arch=""
  local remote_norm_arch=""

  require_cmd curl
  require_cmd ssh
  require_cmd sudo
  require_cmd ip
  require_cmd awk

  local_raw_arch="$(uname -m)"
  local_norm_arch="$(normalize_arch "${local_raw_arch}")"
  log "Localhost architecture: ${local_raw_arch} -> ${local_norm_arch}"

  for host in "${WORKER_HOSTS[@]}"; do
    [[ -n "${host}" ]] || continue
    log "Detecting architecture on ${host} ..."
    remote_raw_arch="$(detect_remote_arch "${host}")"
    remote_norm_arch="$(normalize_arch "${remote_raw_arch}")"
    log "Worker ${host} architecture: ${remote_raw_arch} -> ${remote_norm_arch}"
  done

  prepare_local_runtime_for_k3s
  install_k3s_server_local
  configure_local_kubeconfig

  require_cmd kubectl
  wait_for_local_k3s_api

  control_plane_ip="$(get_control_plane_ip)"
  [[ -n "${control_plane_ip}" ]] || fail "Unable to determine control plane IPv4 address. Set CONTROL_PLANE_IP explicitly."

  node_token="$(sudo cat /var/lib/rancher/k3s/server/node-token)"
  [[ -n "${node_token}" ]] || fail "Unable to read k3s node token."

  log "Control plane endpoint: https://${control_plane_ip}:6443"

  for host in "${WORKER_HOSTS[@]}"; do
    [[ -n "${host}" ]] || continue
    install_k3s_agent_remote "${host}" "${control_plane_ip}" "${node_token}"
  done

  wait_for_nodes_ready "$((1 + ${#WORKER_HOSTS[@]}))"
  kubectl get nodes -L kubernetes.io/arch -o wide

  configure_registry_auth
  deploy_orchestrator
  verify_aarnn_network_operational

  log "Done."
  log "Orchestrator + web-ui status:"
  kubectl -n "${NAMESPACE}" get deploy,pods,svc -l app=neuromorphic -o wide
  log "To open the UI locally: kubectl -n ${NAMESPACE} port-forward svc/web-ui ${WEB_UI_LOCAL_PORT}:8080"
}

main() {
  parse_args "$@"
  validate_config

  case "${ACTION}" in
    deploy)
      deploy_cluster
      ;;
    delete)
      # Deletion only needs SSH and sudo locally, plus kubectl if we can reach
      # the control plane for workload cleanup.
      require_cmd ssh
      require_cmd sudo
      delete_cluster
      ;;
    *)
      fail "Unsupported action: ${ACTION}"
      ;;
  esac
}

main "$@"
