from __future__ import annotations

from dataclasses import dataclass

@dataclass
class RuntimeMetrics:
    error_rate: float
    latency_ms_p95: float
    drift_score: float

def detect_anomaly(metrics: RuntimeMetrics) -> bool:
    return metrics.error_rate > 0.05 or metrics.latency_ms_p95 > 500 or metrics.drift_score > 0.8

def decide_action(metrics: RuntimeMetrics) -> dict:
    if detect_anomaly(metrics):
        return {
            "anomaly": True,
            "actions": ["trigger-training-pipeline", "build-container", "deploy-shadow", "run-experiment"],
        }
    if metrics.error_rate < 0.01 and metrics.latency_ms_p95 < 100:
        return {"anomaly": False, "actions": ["accelerate-rollout"]}
    return {"anomaly": False, "actions": ["continue-observation"]}

if __name__ == "__main__":
    print(decide_action(RuntimeMetrics(error_rate=0.08, latency_ms_p95=640, drift_score=0.21)))
