#!/usr/bin/env bash
# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
set -euo pipefail

ENVIRONMENT="${1:-developer}"

if [[ ! -d "deploy/overlays/${ENVIRONMENT}" ]]; then
  echo "Unknown overlay: ${ENVIRONMENT}" >&2
  exit 1
fi

kubectl apply -k "deploy/overlays/${ENVIRONMENT}"
echo "Applied deploy/overlays/${ENVIRONMENT}"
