from __future__ import annotations

import json
from dataclasses import dataclass, asdict

@dataclass
class VariantMetrics:
    name: str
    conversions: int
    impressions: int
    latency_ms_p95: float

    @property
    def conversion_rate(self) -> float:
        return self.conversions / self.impressions if self.impressions else 0.0

def select_winner(a: VariantMetrics, b: VariantMetrics) -> dict:
    a_score = a.conversion_rate - (a.latency_ms_p95 / 10000.0)
    b_score = b.conversion_rate - (b.latency_ms_p95 / 10000.0)
    winner = a.name if a_score >= b_score else b.name
    return {
        "variant_a": asdict(a),
        "variant_b": asdict(b),
        "a_score": round(a_score, 6),
        "b_score": round(b_score, 6),
        "winner": winner,
        "reason": "higher blended conversion/latency utility score",
    }

if __name__ == "__main__":
    result = select_winner(
        VariantMetrics("A", conversions=410, impressions=1000, latency_ms_p95=125.0),
        VariantMetrics("B", conversions=442, impressions=1000, latency_ms_p95=132.0),
    )
    print(json.dumps(result, indent=2))
