#!/usr/bin/env bash
set -euo pipefail
trap 'printf "[deploy-mixed-cluster] ERROR at line %s\n" "$LINENO" >&2' ERR

# Deploy a mixed-architecture k3s cluster:
# - Control plane on localhost
# - Worker agents over SSH
# - Orchestrator deployment pinned to localhost

WORKER_HOSTS=("192.168.1.60" "192.168.72")
SSH_USER="${SSH_USER:-$USER}"
SSH_PORT="${SSH_PORT:-22}"
SSH_OPTS=(-p "${SSH_PORT}" -o BatchMode=yes -o ConnectTimeout=10 -o StrictHostKeyChecking=accept-new)
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
USE_GHCR_PULL_SECRET="false"
ROLLOUT_TIMEOUT_SECONDS="${ROLLOUT_TIMEOUT_SECONDS:-600}"

usage() {
  cat <<'USAGE'
Usage: scripts/deploy_mixed_cluster.sh [options]

Options:
  --workers <host1,host2>     Comma-separated worker hosts/IPs.
  --ssh-user <user>           SSH user for worker hosts (default: current user).
  --ssh-port <port>           SSH port for worker hosts (default: 22).
  --image <image:tag>         Orchestrator image (default: ghcr.io/neuralmimicry/aarnn_rust:main).
  --namespace <ns>            Kubernetes namespace for orchestrator (default: default).
  --k3s-channel <channel>     k3s release channel (default: stable).
  --wait-timeout <seconds>    Wait timeout for node readiness (default: 300).
  --no-interactive-sudo       Require passwordless sudo on worker hosts.
  --web-ui-local-port <port>  Local port for web-ui operational probe (default: 18080).
  --remote-timeout <seconds>  Timeout for each remote k3s agent install (default: 900).
  --node-wait <seconds>       Timeout waiting for AARNN node registration (default: 180).
  --keep-system-kubelet       Do not stop existing kubelet.service on localhost.
  --keep-remote-system-kubelet
                              Do not stop existing kubelet.service on remote worker hosts.
  --ghcr-username <username>  GHCR username for private image pulls.
  --ghcr-token <token>        GHCR token/PAT for private image pulls.
  --ghcr-email <email>        Email for image pull secret (default: noreply@localhost).
  --ghcr-secret <name>        Kubernetes secret name for GHCR pull creds (default: ghcr-pull-secret).
  --rollout-timeout <seconds> Rollout timeout for orchestrator/web-ui/node workloads (default: 600).
  --help                      Show this help.

Environment overrides are supported for the same fields:
  SSH_USER, SSH_PORT, ORCHESTRATOR_IMAGE, NAMESPACE, INSTALL_K3S_CHANNEL, WAIT_TIMEOUT_SECONDS, INTERACTIVE_SUDO, WEB_UI_LOCAL_PORT, REMOTE_INSTALL_TIMEOUT_SECONDS, AARNN_NODE_WAIT_SECONDS, STOP_SYSTEM_KUBELET, STOP_REMOTE_SYSTEM_KUBELET, GHCR_USERNAME, GHCR_TOKEN, GHCR_EMAIL, GHCR_PULL_SECRET_NAME, ROLLOUT_TIMEOUT_SECONDS
USAGE
}

log() {
  printf '[deploy-mixed-cluster] %s\n' "$*"
}

fail() {
  printf '[deploy-mixed-cluster] ERROR: %s\n' "$*" >&2
  exit 1
}

require_cmd() {
  local cmd="$1"
  command -v "${cmd}" >/dev/null 2>&1 || fail "Required command not found: ${cmd}"
}

normalize_arch() {
  local raw="$1"
  case "${raw}" in
    x86_64|amd64)
      echo "amd64"
      ;;
    aarch64|arm64)
      echo "arm64"
      ;;
    *)
      echo "unknown(${raw})"
      ;;
  esac
}

parse_args() {
  while (($# > 0)); do
    case "$1" in
      --workers)
        shift
        [[ $# -gt 0 ]] || fail "--workers requires a value"
        IFS=',' read -r -a WORKER_HOSTS <<<"$1"
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
        SSH_OPTS=(-p "${SSH_PORT}" -o BatchMode=yes -o ConnectTimeout=10 -o StrictHostKeyChecking=accept-new)
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

get_control_plane_ip() {
  if [[ -n "${CONTROL_PLANE_IP:-}" ]]; then
    echo "${CONTROL_PLANE_IP}"
    return 0
  fi

  ip -4 route get 1.1.1.1 2>/dev/null | awk '
    {
      for (i=1; i<=NF; i++) {
        if ($i == "src" && (i+1) <= NF) {
          print $(i+1)
          exit
        }
      }
    }
  '
}

install_k3s_server_local() {
  local node_name
  node_name="$(hostname -s)"

  if command -v k3s >/dev/null 2>&1; then
    log "k3s already installed on localhost; ensuring service is running."
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
    return 0
  fi

  log "Installing k3s server on localhost (${node_name}) via channel '${INSTALL_K3S_CHANNEL}'."
  curl -sfL https://get.k3s.io | sudo INSTALL_K3S_CHANNEL="${INSTALL_K3S_CHANNEL}" INSTALL_K3S_EXEC="server --node-name ${node_name} --write-kubeconfig-mode 644" sh -
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

configure_local_kubeconfig() {
  mkdir -p "${HOME}/.kube"
  sudo cp /etc/rancher/k3s/k3s.yaml "${HOME}/.kube/config"
  sudo chown "${USER}:${USER}" "${HOME}/.kube/config"
  export KUBECONFIG="${HOME}/.kube/config"
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

install_k3s_agent_remote() {
  local host="$1"
  local control_plane_ip="$2"
  local node_token="$3"
  local remote_cmd_no_pass
  local remote_cmd_with_pass
  local remote_script
  local remote_script_q
  local remote_diag_cmd_no_pass
  local remote_diag_cmd_with_pass
  local sudo_password
  local validated=0
  local attempt

  log "Installing/refreshing k3s agent on ${host}."
  remote_script="$(cat <<'REMOTE'
set -euo pipefail
command -v curl >/dev/null 2>&1 || { echo "curl is required on worker host." >&2; exit 1; }

if systemctl is-active --quiet kubelet; then
  if [ "${STOP_REMOTE_SYSTEM_KUBELET:-true}" = "true" ]; then
    systemctl stop kubelet || true
  else
    echo "kubelet.service is active on worker and conflicts with k3s-agent ports 10248/10250." >&2
    exit 1
  fi
fi

systemctl stop k3s-agent >/dev/null 2>&1 || true

# Clear any stale listeners that would block kubelet inside k3s-agent.
if ss -lntp '( sport = :10248 or sport = :10250 )' 2>/dev/null | grep -q LISTEN; then
  if [ "${STOP_REMOTE_SYSTEM_KUBELET:-true}" = "true" ]; then
    systemctl stop kubelet >/dev/null 2>&1 || true
  fi
fi

curl -sfL https://get.k3s.io | sh -
systemctl enable --now k3s-agent || true
for _ in $(seq 1 30); do
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
  printf -v remote_script_q "%q" "${remote_script}"

  printf -v remote_cmd_no_pass \
    "LC_ALL=C LANG=C sudo -n env INSTALL_K3S_CHANNEL=%q K3S_URL=%q K3S_TOKEN=%q STOP_REMOTE_SYSTEM_KUBELET=%q bash -lc %s" \
    "${INSTALL_K3S_CHANNEL}" "https://${control_plane_ip}:6443" "${node_token}" "${STOP_REMOTE_SYSTEM_KUBELET}" "${remote_script_q}"
  printf -v remote_diag_cmd_no_pass \
    "LC_ALL=C LANG=C sudo -n bash -lc %q" \
    "ss -lntp '( sport = :10248 or sport = :10250 )' || true; systemctl status kubelet --no-pager -l || true; systemctl status k3s-agent --no-pager -l || true; journalctl -u k3s-agent -n 120 --no-pager || true"

  if ssh "${SSH_OPTS[@]}" "${SSH_USER}@${host}" "LC_ALL=C LANG=C sudo -n true" >/dev/null 2>&1; then
    if command -v timeout >/dev/null 2>&1; then
      if ! timeout "${REMOTE_INSTALL_TIMEOUT_SECONDS}" ssh "${SSH_OPTS[@]}" "${SSH_USER}@${host}" "${remote_cmd_no_pass}"; then
        log "Failed to install/start k3s-agent on ${host}; collecting diagnostics."
        ssh "${SSH_OPTS[@]}" "${SSH_USER}@${host}" "${remote_diag_cmd_no_pass}" || true
        fail "k3s-agent installation failed on ${host}."
      fi
    else
      if ! ssh "${SSH_OPTS[@]}" "${SSH_USER}@${host}" "${remote_cmd_no_pass}"; then
        log "Failed to install/start k3s-agent on ${host}; collecting diagnostics."
        ssh "${SSH_OPTS[@]}" "${SSH_USER}@${host}" "${remote_diag_cmd_no_pass}" || true
        fail "k3s-agent installation failed on ${host}."
      fi
    fi
    log "k3s agent ready on ${host}."
    return 0
  fi

  if [[ "${INTERACTIVE_SUDO}" != "true" ]]; then
    fail "Host ${host} requires sudo password. Re-run without --no-interactive-sudo or configure passwordless sudo."
  fi

  for attempt in 1 2 3; do
    log "Host ${host} requires sudo password; enter it locally (hidden input)."
    read -rsp "[deploy-mixed-cluster] sudo password for ${SSH_USER}@${host}: " sudo_password
    echo

    if printf '%s\n' "${sudo_password}" | ssh "${SSH_OPTS[@]}" "${SSH_USER}@${host}" "LC_ALL=C LANG=C sudo -S -p '' -k true" >/dev/null 2>&1; then
      validated=1
      break
    fi
    log "Incorrect sudo password for ${host} (attempt ${attempt}/3)."
  done

  if [[ ${validated} -ne 1 ]]; then
    fail "Failed to validate sudo password on ${host} after 3 attempts."
  fi

  printf -v remote_cmd_with_pass \
    "LC_ALL=C LANG=C sudo -S -p '' env INSTALL_K3S_CHANNEL=%q K3S_URL=%q K3S_TOKEN=%q STOP_REMOTE_SYSTEM_KUBELET=%q bash -lc %s" \
    "${INSTALL_K3S_CHANNEL}" "https://${control_plane_ip}:6443" "${node_token}" "${STOP_REMOTE_SYSTEM_KUBELET}" "${remote_script_q}"
  printf -v remote_diag_cmd_with_pass \
    "LC_ALL=C LANG=C sudo -S -p '' bash -lc %q" \
    "ss -lntp '( sport = :10248 or sport = :10250 )' || true; systemctl status kubelet --no-pager -l || true; systemctl status k3s-agent --no-pager -l || true; journalctl -u k3s-agent -n 120 --no-pager || true"

  if command -v timeout >/dev/null 2>&1; then
    if ! printf '%s\n' "${sudo_password}" | timeout "${REMOTE_INSTALL_TIMEOUT_SECONDS}" ssh "${SSH_OPTS[@]}" "${SSH_USER}@${host}" "${remote_cmd_with_pass}"; then
      log "Failed to install/start k3s-agent on ${host}; collecting diagnostics."
      printf '%s\n' "${sudo_password}" | ssh "${SSH_OPTS[@]}" "${SSH_USER}@${host}" "${remote_diag_cmd_with_pass}" || true
      fail "k3s-agent installation failed on ${host}."
    fi
  else
    if ! printf '%s\n' "${sudo_password}" | ssh "${SSH_OPTS[@]}" "${SSH_USER}@${host}" "${remote_cmd_with_pass}"; then
      log "Failed to install/start k3s-agent on ${host}; collecting diagnostics."
      printf '%s\n' "${sudo_password}" | ssh "${SSH_OPTS[@]}" "${SSH_USER}@${host}" "${remote_diag_cmd_with_pass}" || true
      fail "k3s-agent installation failed on ${host}."
    fi
  fi

  unset sudo_password
  log "k3s agent ready on ${host}."
}

wait_for_nodes_ready() {
  local expected_count="$1"
  local elapsed=0

  while (( elapsed < WAIT_TIMEOUT_SECONDS )); do
    local nodes_output
    local count
    local ready
    nodes_output="$(kubectl get nodes --no-headers 2>/dev/null || true)"
    count="$(awk 'NF>0 {c++} END{print c+0}' <<<"${nodes_output}")"
    ready="$(awk '$2 ~ /^Ready/ {c++} END{print c+0}' <<<"${nodes_output}")"

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
    USE_GHCR_PULL_SECRET="true"
  else
    USE_GHCR_PULL_SECRET="false"
    if [[ "${ORCHESTRATOR_IMAGE}" == ghcr.io/* ]]; then
      log "No GHCR credentials provided. If image is private, set GHCR_USERNAME and GHCR_TOKEN (or use --ghcr-username/--ghcr-token)."
    fi
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
  local control_plane_node_name
  local sa_pull_secret_yaml=""
  local pod_pull_secret_yaml=""
  control_plane_node_name="$(hostname -s)"

  if [[ "${USE_GHCR_PULL_SECRET}" == "true" ]]; then
    sa_pull_secret_yaml=$'imagePullSecrets:\n  - name: '"${GHCR_PULL_SECRET_NAME}"
    pod_pull_secret_yaml=$'      imagePullSecrets:\n        - name: '"${GHCR_PULL_SECRET_NAME}"
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
          args: ["--orchestrator", "--grpc-addr", "0.0.0.0:50051"]
          ports:
            - containerPort: 50051
            - containerPort: 50050
              protocol: UDP
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
          command: ["./web_ui"]
          args: ["--listen", "0.0.0.0:8080", "--orchestrator", "http://orchestrator:50051"]
          ports:
            - containerPort: 8080
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
          args: ["--node", "--orchestrator-addr", "http://orchestrator:50051", "--brain-id", "cluster"]
EOF

  rollout_or_fail deployment orchestrator "${ROLLOUT_TIMEOUT_SECONDS}s"
  rollout_or_fail deployment web-ui "${ROLLOUT_TIMEOUT_SECONDS}s"
  rollout_or_fail daemonset aarnn-node "${ROLLOUT_TIMEOUT_SECONDS}s"
}

verify_aarnn_network_operational() {
  local orchestrator_eps
  local webui_eps
  local pf_pid=""
  local pf_log
  local status_log
  local probe_ok=0
  local elapsed=0
  local expected_nodes
  local registered_nodes=-1
  local status_preview

  require_cmd curl
  require_cmd python3
  expected_nodes="${#WORKER_HOSTS[@]}"

  orchestrator_eps="$(kubectl -n "${NAMESPACE}" get endpoints orchestrator -o jsonpath='{.subsets[*].addresses[*].ip}' 2>/dev/null || true)"
  webui_eps="$(kubectl -n "${NAMESPACE}" get endpoints web-ui -o jsonpath='{.subsets[*].addresses[*].ip}' 2>/dev/null || true)"

  [[ -n "${orchestrator_eps// /}" ]] || fail "Orchestrator service has no endpoints."
  [[ -n "${webui_eps// /}" ]] || fail "Web UI service has no endpoints."

  pf_log="$(mktemp)"
  status_log="$(mktemp)"

  kubectl -n "${NAMESPACE}" port-forward svc/web-ui "${WEB_UI_LOCAL_PORT}:8080" >"${pf_log}" 2>&1 &
  pf_pid=$!

  while (( elapsed < AARNN_NODE_WAIT_SECONDS )); do
    if curl -fsS --max-time 5 "http://127.0.0.1:${WEB_UI_LOCAL_PORT}/api/status" >"${status_log}" 2>/dev/null; then
      probe_ok=1
      registered_nodes="$(python3 - "${status_log}" <<'PY'
import json
import sys

path = sys.argv[1]
try:
    with open(path, "r", encoding="utf-8") as f:
        data = json.load(f)
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
        break
      fi
    fi
    sleep 2
    elapsed=$((elapsed + 2))
  done

  if [[ -n "${pf_pid}" ]]; then
    kill "${pf_pid}" >/dev/null 2>&1 || true
    wait "${pf_pid}" 2>/dev/null || true
  fi

  if [[ ${probe_ok} -ne 1 ]]; then
    cat "${pf_log}" >&2 || true
    rm -f "${pf_log}" "${status_log}"
    fail "AARNN operational probe failed: GET /api/status from web-ui did not succeed."
  fi

  if [[ ! "${registered_nodes}" =~ ^[0-9]+$ ]] || (( registered_nodes < expected_nodes )); then
    status_preview="$(head -c 320 "${status_log}" | tr '\n' ' ')"
    rm -f "${pf_log}" "${status_log}"
    kubectl -n "${NAMESPACE}" get pods -l role=node -o wide || true
    fail "AARNN node registration incomplete: expected >= ${expected_nodes}, observed=${registered_nodes}. Status sample: ${status_preview}"
  fi

  status_preview="$(head -c 320 "${status_log}" | tr '\n' ' ')"
  rm -f "${pf_log}" "${status_log}"

  log "AARNN operational probe succeeded via web-ui (/api/status), registered nodes=${registered_nodes}."
  log "Status sample: ${status_preview}"
}

main() {
  parse_args "$@"

  require_cmd curl
  require_cmd ssh
  require_cmd sudo
  require_cmd ip
  require_cmd awk

  local local_raw_arch
  local local_norm_arch
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

  local control_plane_ip
  control_plane_ip="$(get_control_plane_ip)"
  [[ -n "${control_plane_ip}" ]] || fail "Unable to determine control plane IPv4 address. Set CONTROL_PLANE_IP explicitly."

  local node_token
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

main "$@"
