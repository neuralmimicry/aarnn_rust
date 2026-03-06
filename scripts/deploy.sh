#!/usr/bin/env bash
set -euo pipefail

ENVIRONMENT="${1:-developer}"

if [[ ! -d "deploy/overlays/${ENVIRONMENT}" ]]; then
  echo "Unknown overlay: ${ENVIRONMENT}" >&2
  exit 1
fi

kubectl apply -k "deploy/overlays/${ENVIRONMENT}"
echo "Applied deploy/overlays/${ENVIRONMENT}"
