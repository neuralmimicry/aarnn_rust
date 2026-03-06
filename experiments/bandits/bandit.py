from __future__ import annotations

import json
import math
from dataclasses import dataclass

@dataclass
class Arm:
    name: str
    reward_sum: float
    pulls: int

    @property
    def mean_reward(self) -> float:
        return self.reward_sum / self.pulls if self.pulls else 0.0

def ucb1_score(arm: Arm, total_pulls: int) -> float:
    if arm.pulls == 0:
        return float("inf")
    return arm.mean_reward + math.sqrt((2.0 * math.log(max(total_pulls, 1))) / arm.pulls)

def choose_arm(arms: list[Arm]) -> dict:
    total_pulls = sum(a.pulls for a in arms)
    ranked = sorted(
        [{"name": a.name, "score": ucb1_score(a, total_pulls), "mean_reward": a.mean_reward, "pulls": a.pulls} for a in arms],
        key=lambda x: x["score"],
        reverse=True,
    )
    return {"selected": ranked[0]["name"], "ranking": ranked}

if __name__ == "__main__":
    result = choose_arm(
        [
            Arm("variant-a", reward_sum=42.0, pulls=100),
            Arm("variant-b", reward_sum=55.0, pulls=120),
            Arm("variant-c", reward_sum=18.0, pulls=20),
        ]
    )
    print(json.dumps(result, indent=2))
