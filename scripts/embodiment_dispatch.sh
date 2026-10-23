#!/usr/bin/env bash
# Early routing only. Canonical argument validation, security gates and lifecycle
# remain in scripts/run_sim.sh. Source before any legacy build/hardware setup.
aarnn_dispatch_embodiment() {
  local default_target="$1" policy="$2" root="$3"
  shift 3
  local argv=("$@") spec="" has_spec=0 has_target=0 has_control=0 i=0
  while [ "$i" -lt "${#argv[@]}" ]; do
    case "${argv[$i]}" in
      --robots)
        if [ "$((i + 1))" -ge "${#argv[@]}" ]; then echo '--robots requires a profile/count specification' >&2; exit 2; fi
        spec="${argv[$((i + 1))]}"; has_spec=1; i=$((i + 1)) ;;
      --control) has_control=1 ;;
      --sim)
        if [ "$((i + 1))" -ge "${#argv[@]}" ]; then echo '--sim requires a target' >&2; exit 2; fi
        has_target=1; i=$((i + 1)) ;;
    esac
    i=$((i + 1))
  done
  if [ "$policy" = new ] && [ "$has_control" -eq 0 ]; then
    [ "$has_spec" -eq 1 ] || return 0
    if python3 "$root/scripts/robot_profiles.py" has-new "$spec"; then :
    else
      local status=$?
      [ "$status" -eq 1 ] && return 0
      exit "$status"
    fi
  elif [ "$has_spec" -eq 0 ] && [ "$has_target" -eq 0 ]; then
    return 0
  fi
  if [ "$has_target" -eq 1 ]; then exec "$root/scripts/run_sim.sh" "${argv[@]}"; fi
  exec "$root/scripts/run_sim.sh" --sim "$default_target" "${argv[@]}"
}
