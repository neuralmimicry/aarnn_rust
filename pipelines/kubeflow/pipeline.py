# SPDX-FileCopyrightText: masterkiga
# SPDX-License-Identifier: MIT
#
# Licensed under the MIT License. See LICENSE in the project root.
from __future__ import annotations

def pipeline_description() -> dict:
    return {
        "name": "neuromorphic-training-pipeline",
        "steps": [
            "load-dataset",
            "train-model",
            "evaluate-model",
            "publish-artifact",
        ],
    }

if __name__ == "__main__":
    print(pipeline_description())
