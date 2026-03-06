from __future__ import annotations

from dataclasses import dataclass

@dataclass
class ExperimentSpec:
    name: str
    strategy: str
    primary: str
    candidate: str

def reconcile_experiment(spec: ExperimentSpec) -> dict:
    if spec.strategy not in {"ab", "bandit", "rl"}:
        raise ValueError(f"Unsupported strategy: {spec.strategy}")
    return {
        "resource": spec.name,
        "strategy": spec.strategy,
        "stable_service": spec.primary,
        "candidate_service": spec.candidate,
        "status": "configured",
    }

if __name__ == "__main__":
    print(reconcile_experiment(ExperimentSpec("ab-test", "ab", "stable-svc", "candidate-svc")))
