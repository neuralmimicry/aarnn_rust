#!/usr/bin/env bash

set -euo pipefail

uid="$(id -u)"
conf_dir="${HOME}/.config/containers"
# Multiple matrix jobs can share one self-hosted runner and therefore the
# same run id, attempt, and uid. Include a deterministic short hash of the
# job identity and process id so their rootless overlay stores can never race
# during setup or cleanup while keeping Podman's runroot below its 50-character
# limit (REQ-CI-001).
job_identity="${GITHUB_JOB:-job}-${BASHPID}"
job_key="$(printf '%s' "${job_identity}" | sha256sum | cut -c1-12)"
runtime_dir="/tmp/aarnn-pr-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}-${uid}-${job_key}"
storage_root="/tmp/aarnn-ps-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}-${uid}-${job_key}"
podman_tmp="/tmp/aarnn-pt-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}-${uid}-${job_key}"

mkdir -p "${conf_dir}"
rm -rf "${runtime_dir}" "${storage_root}" "${podman_tmp}"
mkdir -p "${runtime_dir}" "${storage_root}" "${podman_tmp}"
chmod 0700 "${runtime_dir}" "${storage_root}" "${podman_tmp}"

printf '[storage]\ndriver = "overlay"\nrunroot = "%s"\ngraphroot = "%s"\n' \
  "${runtime_dir}" "${storage_root}" > "${conf_dir}/storage.conf"

if command -v runc >/dev/null 2>&1; then
  oci_runtime="$(command -v runc)"
elif command -v crun >/dev/null 2>&1; then
  oci_runtime="$(command -v crun)"
else
  echo "Neither runc nor crun is available." >&2
  exit 1
fi
printf '[engine]\nruntime = "%s"\ncgroup_manager = "cgroupfs"\n' \
  "${oci_runtime}" > "${conf_dir}/containers.conf"

# Podman can reuse the user's existing libpod database even when storage.conf
# changes. Explicit CLI roots prevent build layers from returning to the
# persistent rootless store under ~/.local/share/containers.
podman_path="$(command -v podman)"
wrapper_dir="${RUNNER_TEMP}/podman-bin"
wrapper="${wrapper_dir}/podman"
mkdir -p "${wrapper_dir}"
cat > "${wrapper}" <<EOF
#!/usr/bin/env bash
exec "${podman_path}" \\
  --root "${storage_root}" \\
  --runroot "${runtime_dir}" \\
  --tmpdir "${podman_tmp}" \\
  "\$@"
EOF
chmod 0755 "${wrapper}"

for entry in \
  "XDG_RUNTIME_DIR=${runtime_dir}" \
  "CONTAINERS_STORAGE_CONF=${conf_dir}/storage.conf" \
  "CONTAINERS_CONF=${conf_dir}/containers.conf" \
  "AARNN_PODMAN_ROOT=${storage_root}" \
  "AARNN_PODMAN_RUNROOT=${runtime_dir}" \
  "AARNN_PODMAN_TMPDIR=${podman_tmp}" \
  "PATH=${wrapper_dir}:${PATH}"; do
  echo "${entry}" >> "${GITHUB_ENV}"
done

export XDG_RUNTIME_DIR="${runtime_dir}"
export CONTAINERS_STORAGE_CONF="${conf_dir}/storage.conf"
export CONTAINERS_CONF="${conf_dir}/containers.conf"
export PATH="${wrapper_dir}:${PATH}"
podman info >/dev/null
