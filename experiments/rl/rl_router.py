from __future__ import annotations

import json
from dataclasses import dataclass

@dataclass
class RouteState:
    name: str
    q_value: float

def epsilon_greedy(routes: list[RouteState], epsilon: float = 0.1) -> dict:
    ordered = sorted(routes, key=lambda r: r.q_value, reverse=True)
    return {
        "epsilon": epsilon,
        "exploit_choice": ordered[0].name,
        "exploration_pool": [r.name for r in ordered[1:]],
        "ranking": [{"name": r.name, "q_value": r.q_value} for r in ordered],
    }

if __name__ == "__main__":
    result = epsilon_greedy(
        [RouteState("stable", 0.82), RouteState("candidate-a", 0.85), RouteState("candidate-b", 0.78)],
        epsilon=0.05,
    )
    print(json.dumps(result, indent=2))
