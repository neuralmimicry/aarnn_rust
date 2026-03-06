from __future__ import annotations

import argparse
import json
import math
from pathlib import Path
from statistics import fmean

DEFAULT_DATA = [0.05, 0.2, 0.35, 0.4, 0.55, 0.7, 0.85]

def train_linear_threshold_model(data: list[float]) -> dict:
    mean_value = fmean(data)
    threshold = round(mean_value, 6)
    weight = round(1.0 / max(threshold, 1e-6), 6)
    return {
        "model_type": "threshold-linear",
        "threshold": threshold,
        "weight": weight,
        "training_examples": len(data),
        "mean_input": mean_value,
    }

def main() -> int:
    parser = argparse.ArgumentParser(description="Train a simple demo model artefact.")
    parser.add_argument("--output", default="ml/models/demo_model.json", help="Path to model artefact JSON.")
    parser.add_argument("--data", nargs="*", type=float, help="Optional numeric training inputs.")
    args = parser.parse_args()

    data = args.data or DEFAULT_DATA
    model = train_linear_threshold_model(data)

    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(model, indent=2))
    print(f"Wrote model artefact to {output}")
    print(json.dumps(model, indent=2))
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
