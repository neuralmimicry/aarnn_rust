from __future__ import annotations

import json
from pathlib import Path
from typing import List

from fastapi import FastAPI, HTTPException
from pydantic import BaseModel, Field

MODEL_PATH = Path("ml/models/demo_model.json")

app = FastAPI(title="Neuromorphic Demo Inference API", version="1.0.0")

class PredictionRequest(BaseModel):
    inputs: List[float] = Field(..., min_length=1)

class PredictionResponse(BaseModel):
    score: float
    fired: bool
    threshold: float
    model_type: str

def load_model() -> dict:
    if not MODEL_PATH.exists():
        return {
            "model_type": "threshold-linear",
            "threshold": 0.5,
            "weight": 2.0,
        }
    return json.loads(MODEL_PATH.read_text())

@app.get("/healthz")
def healthz() -> dict:
    model = load_model()
    return {"status": "ok", "model_type": model["model_type"], "model_path": str(MODEL_PATH)}

@app.post("/predict", response_model=PredictionResponse)
def predict(request: PredictionRequest) -> PredictionResponse:
    model = load_model()
    threshold = float(model["threshold"])
    weight = float(model["weight"])
    score = sum(request.inputs) / len(request.inputs) * weight
    return PredictionResponse(
        score=score,
        fired=score >= threshold,
        threshold=threshold,
        model_type=model["model_type"],
    )

@app.get("/model")
def model_info() -> dict:
    return load_model()
