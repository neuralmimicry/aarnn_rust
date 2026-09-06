//! Reproducible scientific/numerical validation reports.
//!
//! Implementation determinism, event timing and biological/behavioural
//! validity are separate claims. This module records the reference profile,
//! tolerances and measured error so a passing digest is not misreported as a
//! biological validation result.

use crate::deterministic::{StateDigest, StateDigestBuilder};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ToleranceProfile {
    pub absolute: f64,
    pub relative: f64,
    pub max_spike_timing_ticks: u64,
    pub minimum_correlation: f64,
}

impl ToleranceProfile {
    pub const DETERMINISTIC: Self = Self {
        absolute: 0.0,
        relative: 0.0,
        max_spike_timing_ticks: 0,
        minimum_correlation: 1.0,
    };
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationReport {
    pub schema_version: u32,
    pub dataset_id: String,
    pub reference_profile: String,
    pub reference_digest: StateDigest,
    pub candidate_digest: StateDigest,
    pub max_absolute_error: f64,
    pub max_relative_error: f64,
    pub spike_timing_error_ticks: u64,
    pub correlation: f64,
    pub implementation_pass: bool,
    pub numerical_pass: bool,
    pub behavioural_validity_assessed: bool,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ScientificValidationError {
    #[error("validation vectors have different lengths")]
    LengthMismatch,
    #[error("validation dataset identifier is empty")]
    EmptyDataset,
    #[error("non-finite validation value")]
    NonFinite,
    #[error("reference and candidate digests differ in deterministic profile")]
    DigestMismatch,
    #[error("validation tolerance values must be finite and non-negative")]
    InvalidTolerance,
}

pub fn compare(
    dataset_id: impl Into<String>,
    reference_profile: impl Into<String>,
    reference: &[f64],
    candidate: &[f64],
    reference_digest: StateDigest,
    candidate_digest: StateDigest,
    tolerance: ToleranceProfile,
) -> Result<ValidationReport, ScientificValidationError> {
    compare_with_spike_timing(
        dataset_id,
        reference_profile,
        reference,
        candidate,
        reference_digest,
        candidate_digest,
        tolerance,
        0,
    )
}

/// Compare a candidate while retaining the independently measured spike-time
/// error.  Numerical agreement alone cannot hide a timing breach.
pub fn compare_with_spike_timing(
    dataset_id: impl Into<String>,
    reference_profile: impl Into<String>,
    reference: &[f64],
    candidate: &[f64],
    reference_digest: StateDigest,
    candidate_digest: StateDigest,
    tolerance: ToleranceProfile,
    spike_timing_error_ticks: u64,
) -> Result<ValidationReport, ScientificValidationError> {
    let dataset_id = dataset_id.into();
    let reference_profile = reference_profile.into();
    if dataset_id.trim().is_empty() {
        return Err(ScientificValidationError::EmptyDataset);
    }
    if !tolerance.absolute.is_finite()
        || !tolerance.relative.is_finite()
        || tolerance.absolute < 0.0
        || tolerance.relative < 0.0
        || !tolerance.minimum_correlation.is_finite()
        || !(-1.0..=1.0).contains(&tolerance.minimum_correlation)
    {
        return Err(ScientificValidationError::InvalidTolerance);
    }
    if reference.len() != candidate.len() {
        return Err(ScientificValidationError::LengthMismatch);
    }
    if reference
        .iter()
        .chain(candidate)
        .any(|value| !value.is_finite())
    {
        return Err(ScientificValidationError::NonFinite);
    }
    let mut max_absolute_error: f64 = 0.0;
    let mut max_relative_error: f64 = 0.0;
    for (left, right) in reference.iter().zip(candidate) {
        let absolute = (left - right).abs();
        max_absolute_error = max_absolute_error.max(absolute);
        max_relative_error = max_relative_error.max(absolute / left.abs().max(1e-12));
    }
    let correlation = pearson(reference, candidate);
    let implementation_pass = reference_digest == candidate_digest;
    if reference_profile == "DeterministicReference" && !implementation_pass {
        return Err(ScientificValidationError::DigestMismatch);
    }
    let numerical_pass = max_absolute_error <= tolerance.absolute
        && max_relative_error <= tolerance.relative
        && spike_timing_error_ticks <= tolerance.max_spike_timing_ticks;
    Ok(ValidationReport {
        schema_version: 1,
        dataset_id,
        reference_profile,
        reference_digest,
        candidate_digest,
        max_absolute_error,
        max_relative_error,
        spike_timing_error_ticks,
        correlation,
        implementation_pass,
        numerical_pass: numerical_pass && correlation >= tolerance.minimum_correlation,
        behavioural_validity_assessed: false,
        limitations: vec!["Behavioural validity requires a modality-specific scientific dataset and is not inferred from implementation agreement.".to_owned()],
    })
}

pub fn digest_values(values: &[f64]) -> StateDigest {
    let mut bytes = Vec::with_capacity(values.len() * 8);
    for value in values {
        bytes.extend_from_slice(&value.to_bits().to_be_bytes());
    }
    let mut digest = StateDigestBuilder::default();
    digest.add_domain("scientific-values:v1", bytes);
    digest.finish()
}

fn pearson(reference: &[f64], candidate: &[f64]) -> f64 {
    if reference.is_empty() {
        return 1.0;
    }
    if reference == candidate {
        return 1.0;
    }
    let mean_left = reference.iter().sum::<f64>() / reference.len() as f64;
    let mean_right = candidate.iter().sum::<f64>() / candidate.len() as f64;
    let mut numerator = 0.0;
    let mut left_norm = 0.0;
    let mut right_norm = 0.0;
    for (left, right) in reference.iter().zip(candidate) {
        let a = left - mean_left;
        let b = right - mean_right;
        numerator += a * b;
        left_norm += a * a;
        right_norm += b * b;
    }
    if left_norm == 0.0 && right_norm == 0.0 {
        1.0
    } else if left_norm == 0.0 || right_norm == 0.0 {
        0.0
    } else {
        // The accumulated dot products can round a mathematically identical
        // vector to 0.9999999999999998.  A deterministic-reference fixture
        // must not fail merely because of that last floating-point ulp, while
        // still preserving a bounded correlation metric for non-identical
        // candidates.
        (numerator / (left_norm.sqrt() * right_norm.sqrt())).clamp(-1.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_separates_exact_implementation_from_biological_validity() {
        let values = [0.0, 1.0, 0.5];
        let digest = digest_values(&values);
        let report = compare(
            "fixture-1",
            "DeterministicReference",
            &values,
            &values,
            digest,
            digest,
            ToleranceProfile::DETERMINISTIC,
        )
        .unwrap();
        assert!(report.implementation_pass && report.numerical_pass);
        assert!(!report.behavioural_validity_assessed);
        assert!(!report.limitations.is_empty());
    }

    #[test]
    fn fast_profile_reports_tolerance_breaches_without_calling_them_exact() {
        let reference = [1.0, 2.0];
        let candidate = [1.01, 2.01];
        let report = compare(
            "fixture-2",
            "FastBiological",
            &reference,
            &candidate,
            StateDigest([1; 16]),
            StateDigest([2; 16]),
            ToleranceProfile {
                absolute: 0.02,
                relative: 0.02,
                max_spike_timing_ticks: 1,
                minimum_correlation: 0.9,
            },
        )
        .unwrap();
        assert!(!report.implementation_pass);
        assert!(report.numerical_pass);
    }

    #[test]
    fn numerical_validation_requires_both_absolute_and_relative_bounds() {
        let report = compare(
            "fixture-3",
            "FastBiological",
            &[1.0, 100.0],
            &[1.1, 100.0],
            StateDigest([1; 16]),
            StateDigest([2; 16]),
            ToleranceProfile {
                absolute: 0.01,
                relative: 0.2,
                max_spike_timing_ticks: 1,
                minimum_correlation: 0.9,
            },
        )
        .unwrap();
        assert!(!report.numerical_pass);
    }

    #[test]
    fn spike_timing_tolerance_is_an_independent_numerical_gate() {
        let report = compare_with_spike_timing(
            "fixture-timing",
            "FastBiological",
            &[0.0, 1.0],
            &[0.0, 1.0],
            StateDigest([1; 16]),
            StateDigest([1; 16]),
            ToleranceProfile {
                absolute: 0.0,
                relative: 0.0,
                max_spike_timing_ticks: 2,
                minimum_correlation: 1.0,
            },
            3,
        )
        .unwrap();
        assert!(!report.numerical_pass);
        assert_eq!(report.spike_timing_error_ticks, 3);
    }

    #[test]
    fn invalid_tolerance_is_rejected_before_comparison() {
        assert!(matches!(
            compare(
                "fixture-4",
                "FastBiological",
                &[1.0],
                &[1.0],
                StateDigest([1; 16]),
                StateDigest([1; 16]),
                ToleranceProfile {
                    absolute: f64::NAN,
                    ..ToleranceProfile::DETERMINISTIC
                },
            ),
            Err(ScientificValidationError::InvalidTolerance)
        ));
    }
}
