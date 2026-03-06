#!/usr/bin/env bash
set -euo pipefail
python ml/training/train.py --output ml/models/demo_model.json "$@"
