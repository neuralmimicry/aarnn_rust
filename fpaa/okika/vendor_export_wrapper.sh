#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  vendor_export_wrapper.sh \
    --kernel-id <id> \
    --manifest <path/to/kernel.okika.json> \
    --xcos <path/to/kernel.sce> \
    --output <path/to/kernel.ahf>

Environment:
  FPAA_VENDOR_EXPORT_CMD
    Shell command template used to invoke your vendor exporter.
    Placeholders supported:
      {kernel_id}   {manifest}   {xcos}   {output}
      {kernel_id_q} {manifest_q} {xcos_q} {output_q}

Example:
  export FPAA_VENDOR_EXPORT_CMD='vendor_cli --manifest {manifest_q} --xcos {xcos_q} --output {output_q}'
USAGE
}

kernel_id=""
manifest=""
xcos=""
output=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --kernel-id)
      kernel_id="${2:-}"
      shift 2
      ;;
    --manifest)
      manifest="${2:-}"
      shift 2
      ;;
    --xcos)
      xcos="${2:-}"
      shift 2
      ;;
    --output)
      output="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown argument '$1'" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if [[ -z "${kernel_id}" || -z "${manifest}" || -z "${xcos}" || -z "${output}" ]]; then
  echo "error: missing required arguments" >&2
  usage >&2
  exit 1
fi

if [[ -z "${FPAA_VENDOR_EXPORT_CMD:-}" ]]; then
  echo "error: FPAA_VENDOR_EXPORT_CMD is not set" >&2
  usage >&2
  exit 1
fi

rendered="$(
python3 - "$FPAA_VENDOR_EXPORT_CMD" "$kernel_id" "$manifest" "$xcos" "$output" <<'PY'
import shlex
import sys

template = sys.argv[1]
kernel_id = sys.argv[2]
manifest = sys.argv[3]
xcos = sys.argv[4]
output = sys.argv[5]
print(
    template.format(
        kernel_id=kernel_id,
        manifest=manifest,
        xcos=xcos,
        output=output,
        kernel_id_q=shlex.quote(kernel_id),
        manifest_q=shlex.quote(manifest),
        xcos_q=shlex.quote(xcos),
        output_q=shlex.quote(output),
    )
)
PY
)"

echo "[vendor-export] ${rendered}"
bash -lc "${rendered}"
