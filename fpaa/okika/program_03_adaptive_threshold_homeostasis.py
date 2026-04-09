#!/usr/bin/env python3
from pathlib import Path
import sys

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str((HERE / "common").resolve()))

from pika_loader import cli_for_fixed_manifest

if __name__ == "__main__":
    raise SystemExit(cli_for_fixed_manifest(HERE / "03_adaptive_threshold_homeostasis.okika.json"))
