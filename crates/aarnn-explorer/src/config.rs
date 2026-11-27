// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Validated explorer configuration (brief Section 19). Unknown fields are
//! rejected; ranges, units and resource consistency are checked; a preset
//! name is expanded into explicit requirements and the expansion is stored
//! with every result.

use serde::{Deserialize, Serialize};

use crate::error::{Result, invalid};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ExplorerConfig {
    pub schema_version: u32,
    pub name: String,
    pub execution: Execution,
    pub resources: Resources,
    pub capture: Capture,
    pub gnat: GnatConfig,
    pub functional: Functional,
    pub resilience: Resilience,
    pub optimisation: Optimisation,
    pub hardware: Hardware,
    #[serde(default)]
    pub ui: Ui,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Execution {
    /// `source`: run the source network's own model through the AARNN Runner.
    pub backend: String,
    /// `inherit`: the source's numeric profile (f64 reference).
    pub numeric_profile: String,
    /// `frozen` (acute) or `source` (keep the network's plasticity settings).
    pub plasticity_policy: String,
    pub seed: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Resources {
    pub total_compute_threads: u32,
    pub analysis_threads: u32,
    pub max_worker_processes: u32,
    pub threads_per_worker: u32,
    pub memory_limit_mib: u64,
    pub disk_budget_mib: u64,
    pub worker_wall_timeout_seconds: u64,
    pub gpu_enabled: bool,
    /// `local` or `slurm`.
    pub launcher: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Capture {
    /// `scientific-isolated` (complete, replayable) or `live-observation`.
    pub mode: String,
    pub require_complete: bool,
    pub max_segment_mib: u64,
    /// `none` is the only implemented codec; `zstd` is rejected until a
    /// codec is added (no silent fallback).
    pub compression: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GnatConfig {
    /// `effective-delivery-v1` (spiking cells) or
    /// `effective-delivery-graded-v1` (adds graded spike-equivalent events).
    pub method: String,
    pub tau_ms: f64,
    /// `fixed` or `development-calibrated`.
    pub threshold_mode: String,
    pub minimum_score: f64,
    pub require_generation_mapping: bool,
    /// `bounded-verified` or `off`.
    pub recurrence_mode: String,
    pub max_motif_candidates: u64,
    #[serde(default = "default_norm")]
    pub norm: String,
}

fn default_norm() -> String {
    "l1-abs".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Functional {
    pub task_suite: String,
    pub minimum_reference_score_ratio: f64,
    pub maximum_additional_latency_ms: f64,
    pub require_all_tasks: bool,
    pub development_split: String,
    pub validation_split: String,
    pub learning_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Resilience {
    pub preset: String,
    pub eligible_population: String,
    pub random_neuron_loss_fraction: f64,
    pub random_synapse_loss_fraction: f64,
    pub targeted_single_faults_required: bool,
    pub minimum_success_probability: f64,
    pub confidence: f64,
    pub confidence_scope: String,
    pub trials_per_sampled_claim: u64,
    pub recovery_policy: String,
    pub recovery_window_ms: f64,
    /// Restrict the eligible fault population to these regions (empty: all).
    #[serde(default)]
    pub fault_regions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Optimisation {
    pub method: String,
    pub proposal_budget: u64,
    pub beam_width: u32,
    pub allow_neuron_removal: bool,
    pub allow_synapse_removal: bool,
    pub allow_verified_relay_replacement: bool,
    pub training_assisted_reduction: bool,
    pub protect_input_output_mapping: bool,
    pub cost_dimensions: Vec<String>,
    /// Declared out-of-contract function: hidden cells whose cell type starts
    /// with one of these prefixes are never proposed for removal. For
    /// function the contract cannot afford to test, e.g. rhythms slower
    /// than its trials (a 10-20 s exploration oscillator under 1 s trials).
    #[serde(default)]
    pub protect_cell_types: Vec<String>,
    /// Declared out-of-contract function, by stable neuron ID.
    #[serde(default)]
    pub protect_neurons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Hardware {
    pub target: String,
    pub cost_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Ui {
    pub listen: String,
    pub maximum_visible_nodes: u64,
    pub maximum_visible_edges: u64,
}

impl Default for Ui {
    fn default() -> Self {
        Ui {
            listen: "127.0.0.1:0".into(),
            maximum_visible_nodes: 20_000,
            maximum_visible_edges: 50_000,
        }
    }
}

pub const COST_DIMENSIONS: &[&str] = &[
    "state_bytes",
    "synapses",
    "neurons",
    "latency_ms",
    "event_rate",
    "memory_bytes",
];

fn one_of(field: &str, value: &str, allowed: &[&str]) -> Result<()> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        invalid(format!("{field} = {value:?}: expected one of {allowed:?}"))
    }
}

fn range(field: &str, value: f64, lo: f64, hi: f64) -> Result<()> {
    if value.is_finite() && value >= lo && value <= hi {
        Ok(())
    } else {
        invalid(format!(
            "{field} = {value}: expected a finite value in [{lo}, {hi}]"
        ))
    }
}

impl ExplorerConfig {
    pub fn parse(text: &str) -> Result<ExplorerConfig> {
        let cfg: ExplorerConfig = toml::from_str(text)
            .map_err(|e| crate::error::ExplorerError::InvalidInput(format!("config: {e}")))?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn load(path: &std::path::Path) -> Result<ExplorerConfig> {
        let text = std::fs::read_to_string(path).map_err(|e| {
            crate::error::ExplorerError::InvalidInput(format!("{}: {e}", path.display()))
        })?;
        Self::parse(&text)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            return invalid(format!(
                "schema_version {} (expected 1)",
                self.schema_version
            ));
        }
        crate::store::check_name(&self.name)?;
        one_of("execution.backend", &self.execution.backend, &["source"])?;
        one_of(
            "execution.numeric_profile",
            &self.execution.numeric_profile,
            &["inherit"],
        )?;
        one_of(
            "execution.plasticity_policy",
            &self.execution.plasticity_policy,
            &["frozen", "source"],
        )?;
        let r = &self.resources;
        if r.total_compute_threads == 0
            || r.analysis_threads == 0
            || r.max_worker_processes == 0
            || r.threads_per_worker == 0
        {
            return invalid("resources: thread and worker counts must be >= 1");
        }
        if r.analysis_threads + r.max_worker_processes * r.threads_per_worker
            > r.total_compute_threads.max(1) * 2
        {
            return invalid(format!(
                "resources: analysis_threads ({}) + max_worker_processes x threads_per_worker ({}) exceeds twice total_compute_threads ({})",
                r.analysis_threads,
                r.max_worker_processes * r.threads_per_worker,
                r.total_compute_threads
            ));
        }
        if r.memory_limit_mib < 64 || r.disk_budget_mib < 16 || r.worker_wall_timeout_seconds == 0 {
            return invalid(
                "resources: memory_limit_mib >= 64, disk_budget_mib >= 16, worker_wall_timeout_seconds >= 1",
            );
        }
        if r.gpu_enabled {
            return invalid(
                "resources.gpu_enabled: no accelerator backend is admitted by the explorer (opt-in GPU is not implemented)",
            );
        }
        one_of("resources.launcher", &r.launcher, &["local", "slurm"])?;
        one_of(
            "capture.mode",
            &self.capture.mode,
            &["scientific-isolated", "live-observation"],
        )?;
        one_of("capture.compression", &self.capture.compression, &["none"]).map_err(|_| {
            crate::error::ExplorerError::Unsupported(format!(
                "capture.compression = {:?}: only \"none\" is implemented",
                self.capture.compression
            ))
        })?;
        if self.capture.max_segment_mib == 0 || self.capture.max_segment_mib > 1024 {
            return invalid("capture.max_segment_mib must be in 1..=1024");
        }
        let g = &self.gnat;
        one_of(
            "gnat.method",
            &g.method,
            &["effective-delivery-v1", "effective-delivery-graded-v1"],
        )?;
        range("gnat.tau_ms", g.tau_ms, 1e-3, 1e4)?;
        one_of(
            "gnat.threshold_mode",
            &g.threshold_mode,
            &["fixed", "development-calibrated"],
        )?;
        if !(g.minimum_score > 0.0 && g.minimum_score < 1.0) {
            return invalid("gnat.minimum_score must be in (0, 1)");
        }
        one_of(
            "gnat.recurrence_mode",
            &g.recurrence_mode,
            &["bounded-verified", "off"],
        )?;
        one_of("gnat.norm", &g.norm, &["l1-abs", "l2-abs"])?;
        let f = &self.functional;
        range(
            "functional.minimum_reference_score_ratio",
            f.minimum_reference_score_ratio,
            0.0,
            1.0,
        )?;
        range(
            "functional.maximum_additional_latency_ms",
            f.maximum_additional_latency_ms,
            0.0,
            1e6,
        )?;
        if f.development_split == f.validation_split {
            return invalid("functional: development_split and validation_split must differ");
        }
        let rs = &self.resilience;
        one_of(
            "resilience.preset",
            &rs.preset,
            &[
                "compact-silicon",
                "resilient-silicon",
                "biological-variability",
                "adaptive-recovery",
                "custom",
            ],
        )?;
        one_of(
            "resilience.eligible_population",
            &rs.eligible_population,
            &[
                "all-internal-neurons",
                "all-internal-synapses",
                "unprotected",
            ],
        )?;
        range(
            "resilience.random_neuron_loss_fraction",
            rs.random_neuron_loss_fraction,
            0.0,
            1.0,
        )?;
        range(
            "resilience.random_synapse_loss_fraction",
            rs.random_synapse_loss_fraction,
            0.0,
            1.0,
        )?;
        range(
            "resilience.minimum_success_probability",
            rs.minimum_success_probability,
            0.0,
            1.0,
        )?;
        if !(rs.confidence > 0.0 && rs.confidence < 1.0) {
            return invalid("resilience.confidence must be in (0, 1)");
        }
        one_of(
            "resilience.confidence_scope",
            &rs.confidence_scope,
            &["simultaneous-required-claims", "per-claim"],
        )?;
        one_of(
            "resilience.recovery_policy",
            &rs.recovery_policy,
            &["none", "plasticity"],
        )?;
        if rs.recovery_policy == "none" && rs.recovery_window_ms != 0.0 {
            return invalid(
                "resilience.recovery_window_ms must be 0 when recovery_policy = \"none\"",
            );
        }
        let o = &self.optimisation;
        one_of(
            "optimisation.method",
            &o.method,
            &["deterministic-greedy", "deterministic-beam"],
        )?;
        if o.proposal_budget == 0 || o.beam_width == 0 {
            return invalid("optimisation: proposal_budget and beam_width must be >= 1");
        }
        if o.protect_cell_types.iter().any(|p| p.is_empty()) {
            return invalid(
                "optimisation.protect_cell_types: an empty prefix would protect every cell",
            );
        }
        if o.allow_verified_relay_replacement {
            return Err(crate::error::ExplorerError::Unsupported(
                "optimisation.allow_verified_relay_replacement: no relay-replacement rule is established for the AARNN graded/spiking cell models (see optimisation-and-hardware.md)".into(),
            ));
        }
        if o.training_assisted_reduction {
            return Err(crate::error::ExplorerError::Unsupported(
                "optimisation.training_assisted_reduction is an extension contract, not implemented".into(),
            ));
        }
        for d in &o.cost_dimensions {
            one_of("optimisation.cost_dimensions[]", d, COST_DIMENSIONS)?;
        }
        if o.cost_dimensions.is_empty() {
            return invalid("optimisation.cost_dimensions must name at least one dimension");
        }
        one_of(
            "hardware.target",
            &self.hardware.target,
            &["aarnn-cpu-native"],
        )?;
        one_of(
            "hardware.cost_mode",
            &self.hardware.cost_mode,
            &["measured-when-supported", "proxy"],
        )?;
        if !self.ui.listen.starts_with("127.0.0.1:") && !self.ui.listen.starts_with("[::1]:") {
            return invalid("ui.listen must be a loopback address (127.0.0.1:PORT or [::1]:PORT)");
        }
        Ok(())
    }

    /// Expanded resilience requirements stored in each result (a preset name
    /// alone is never evidence).
    pub fn expanded_resilience(&self) -> serde_json::Value {
        serde_json::to_value(&self.resilience).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT: &str = include_str!("../../../configs/explorer/default.toml");

    #[test]
    fn default_profile_parses() {
        let c = ExplorerConfig::parse(DEFAULT).unwrap();
        assert_eq!(c.execution.seed, 20261125);
    }

    #[test]
    fn unknown_field_is_rejected() {
        let bad = DEFAULT.replace("[execution]", "[execution]\nmystery = 1");
        assert!(ExplorerConfig::parse(&bad).is_err());
    }

    #[test]
    fn out_of_range_and_unsupported_are_rejected() {
        assert!(ExplorerConfig::parse(&DEFAULT.replace("tau_ms = 5.0", "tau_ms = -1.0")).is_err());
        let z = DEFAULT.replace("compression = \"none\"", "compression = \"zstd\"");
        assert!(matches!(
            ExplorerConfig::parse(&z),
            Err(crate::error::ExplorerError::Unsupported(_))
        ));
        assert!(ExplorerConfig::parse(&DEFAULT.replace("127.0.0.1:0", "0.0.0.0:80")).is_err());
    }
}
