from __future__ import annotations

from dataclasses import dataclass

@dataclass
class ModelSpec:
    name: str
    image: str
    replicas: int = 1

def reconcile_model(spec: ModelSpec) -> dict:
    return {
        "resource": spec.name,
        "action": "apply-deployment",
        "deployment_name": f"{spec.name}-deployment",
        "image": spec.image,
        "replicas": spec.replicas,
        "status": "reconciled",
    }

if __name__ == "__main__":
    print(reconcile_model(ModelSpec(name="neuromorphic-model", image="ghcr.io/neuralmimicry/aarnn_rust:engine")))
