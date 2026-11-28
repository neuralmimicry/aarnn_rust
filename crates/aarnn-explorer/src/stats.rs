// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Exact binomial confidence bounds (brief Section 11).
//!
//! Convention: one-sided lower Clopper-Pearson bound at confidence `c` for
//! `k` successes in `n` independent trials is the `p` solving
//! `P(X >= k | n, p) = 1 - c`, with `p = 0` when `k = 0`. For `k = n` this is
//! `(1 - c)^(1/n)`: 299 of 299 at 95 % gives 0.990030 > 0.99, 298 of 298
//! gives 0.989997 < 0.99 (test `golden_zero_failure_cases`).

/// ln C(n, k) via lgamma-free summation (n up to a few million).
fn ln_choose(n: u64, k: u64) -> f64 {
    let k = k.min(n - k);
    (0..k)
        .map(|i| ((n - i) as f64).ln() - ((i + 1) as f64).ln())
        .sum()
}

/// P(X >= k) for X ~ Binomial(n, p), summed in log space.
pub fn binomial_upper_tail(n: u64, k: u64, p: f64) -> f64 {
    if k == 0 {
        return 1.0;
    }
    if k > n {
        return 0.0;
    }
    if p <= 0.0 {
        return 0.0;
    }
    if p >= 1.0 {
        return 1.0;
    }
    let (lp, lq) = (p.ln(), (1.0 - p).ln());
    let mut lc = ln_choose(n, k);
    let mut terms = Vec::with_capacity((n - k + 1) as usize);
    for x in k..=n {
        terms.push(lc + x as f64 * lp + (n - x) as f64 * lq);
        if x < n {
            lc += ((n - x) as f64).ln() - ((x + 1) as f64).ln();
        }
    }
    let m = terms.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    (m + terms.iter().map(|t| (t - m).exp()).sum::<f64>().ln())
        .exp()
        .min(1.0)
}

/// One-sided lower Clopper-Pearson bound.
pub fn clopper_pearson_lower(successes: u64, trials: u64, confidence: f64) -> f64 {
    assert!(successes <= trials && trials > 0);
    assert!(confidence > 0.0 && confidence < 1.0);
    if successes == 0 {
        return 0.0;
    }
    let alpha = 1.0 - confidence;
    if successes == trials {
        return alpha.powf(1.0 / trials as f64);
    }
    // binomial_upper_tail is increasing in p: bisect.
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if binomial_upper_tail(trials, successes, mid) < alpha {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// Bonferroni allocation of a family-wise confidence across `claims`.
pub fn bonferroni_per_claim_confidence(family_confidence: f64, claims: usize) -> f64 {
    1.0 - (1.0 - family_confidence) / claims.max(1) as f64
}

/// Smallest number of all-success trials whose lower bound exceeds `p_min`.
pub fn zero_failure_trials_needed(p_min: f64, confidence: f64) -> u64 {
    if p_min <= 0.0 {
        return 1;
    }
    ((1.0 - confidence).ln() / p_min.ln()).floor() as u64 + 1
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum ClaimVerdict {
    #[serde(rename = "passed")]
    Passed,
    #[serde(rename = "failed")]
    Failed,
    /// The budget cannot establish the bound either way.
    #[serde(rename = "inconclusive")]
    Inconclusive,
}

/// Verdict for "success probability >= p_min at confidence c": passed when
/// the lower bound clears p_min; failed when even the upper bound (computed
/// as 1 - lower bound of failures) is below p_min; otherwise inconclusive.
pub fn probability_claim(
    successes: u64,
    trials: u64,
    p_min: f64,
    confidence: f64,
) -> (ClaimVerdict, f64, f64) {
    let lower = clopper_pearson_lower(successes, trials, confidence);
    let upper = 1.0 - clopper_pearson_lower(trials - successes, trials, confidence);
    let v = if lower >= p_min {
        ClaimVerdict::Passed
    } else if upper < p_min {
        ClaimVerdict::Failed
    } else {
        ClaimVerdict::Inconclusive
    };
    (v, lower, upper)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_zero_failure_cases() {
        let b299 = clopper_pearson_lower(299, 299, 0.95);
        let b298 = clopper_pearson_lower(298, 298, 0.95);
        assert!(b299 > 0.99 && (b299 - 0.990030).abs() < 1e-6, "{b299}");
        assert!(b298 < 0.99, "{b298}");
        assert_eq!(zero_failure_trials_needed(0.99, 0.95), 299);
    }

    #[test]
    fn interior_bound_matches_reference() {
        // Reference: one-sided 95 % lower Clopper-Pearson for 95/100 is the
        // beta quantile Beta(0.05; 95, 6) = 0.8977466 (scipy.stats.beta.ppf).
        let b = clopper_pearson_lower(95, 100, 0.95);
        assert!((b - 0.8977466).abs() < 1e-6, "{b}");
        // Monotone in successes.
        assert!(clopper_pearson_lower(96, 100, 0.95) > b);
    }

    #[test]
    fn verdicts_cover_pass_fail_inconclusive() {
        assert_eq!(
            probability_claim(299, 299, 0.99, 0.95).0,
            ClaimVerdict::Passed
        );
        assert_eq!(
            probability_claim(50, 100, 0.99, 0.95).0,
            ClaimVerdict::Failed
        );
        assert_eq!(
            probability_claim(10, 10, 0.99, 0.95).0,
            ClaimVerdict::Inconclusive
        );
    }

    #[test]
    fn bonferroni_tightens_per_claim_confidence() {
        assert!((bonferroni_per_claim_confidence(0.95, 5) - 0.99).abs() < 1e-12);
    }
}
