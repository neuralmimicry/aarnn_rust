use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub const DEFAULT_SWARMHPC_ANSIBLE_ROOT: &str =
    "/home/pbisaacs/Developer/swarmhpc/swarmhpc/ansible";

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    Individual,
    Distributed,
    Sharded,
    Grouped,
    Combined,
    Federated,
}

impl ExecutionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Individual => "individual",
            Self::Distributed => "distributed",
            Self::Sharded => "sharded",
            Self::Grouped => "grouped",
            Self::Combined => "combined",
            Self::Federated => "federated",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionScope {
    Auto,
    Node,
    Container,
    System,
    Cluster,
    FederatedCluster,
}

impl ExecutionScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Node => "node",
            Self::Container => "container",
            Self::System => "system",
            Self::Cluster => "cluster",
            Self::FederatedCluster => "federated_cluster",
        }
    }
}

impl Default for ExecutionScope {
    fn default() -> Self {
        Self::Auto
    }
}

fn is_auto_scope(scope: &ExecutionScope) -> bool {
    matches!(scope, ExecutionScope::Auto)
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}

fn is_zero_u64(value: &u64) -> bool {
    *value == 0
}

fn clean_id(raw: &str) -> Option<String> {
    let cleaned = raw
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim_matches('`')
        .trim_matches(|ch| ch == '[' || ch == ']' || ch == '{' || ch == '}')
        .trim();
    (!cleaned.is_empty()).then(|| cleaned.to_string())
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeploymentTransitionPolicy {
    #[serde(skip_serializing_if = "is_false")]
    pub allow_live_transition: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub autonomous: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub permitted_modes: Vec<ExecutionMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_step_time_ms: Option<f32>,
    #[serde(skip_serializing_if = "is_zero_u64")]
    pub cooldown_ms: u64,
}

impl DeploymentTransitionPolicy {
    pub fn normalize(&mut self) {
        if self.autonomous {
            self.allow_live_transition = true;
        }

        let mut seen = HashSet::new();
        self.permitted_modes.retain(|mode| seen.insert(*mode));

        if let Some(target_step_time_ms) = self.target_step_time_ms {
            if !target_step_time_ms.is_finite() || target_step_time_ms <= 0.0 {
                self.target_step_time_ms = None;
            }
        }
    }

    pub fn target_step_time_ms(&self) -> f32 {
        self.target_step_time_ms
            .unwrap_or(10.0)
            .clamp(0.5, 10_000.0)
    }

    pub fn mode_allowed(&self, mode: ExecutionMode) -> bool {
        self.permitted_modes.is_empty() || self.permitted_modes.contains(&mode)
    }

    pub fn is_default(value: &Self) -> bool {
        value == &Self::default()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeploymentConfig {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub modes: Vec<ExecutionMode>,
    #[serde(skip_serializing_if = "is_auto_scope")]
    pub scope: ExecutionScope,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub related_network_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub combined_group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub federation_group: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub infrastructure_roots: Vec<String>,
    #[serde(skip_serializing_if = "is_false")]
    pub autodetect_infrastructure: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub allow_multi_user: bool,
    #[serde(skip_serializing_if = "is_zero")]
    pub max_concurrent_networks: usize,
    #[serde(skip_serializing_if = "is_zero")]
    pub desired_shards: usize,
    #[serde(skip_serializing_if = "DeploymentTransitionPolicy::is_default")]
    pub transition_policy: DeploymentTransitionPolicy,
}

impl DeploymentConfig {
    pub fn add_mode(&mut self, mode: ExecutionMode) {
        if !self.modes.contains(&mode) {
            self.modes.push(mode);
        }
    }

    pub fn has_mode(&self, mode: ExecutionMode) -> bool {
        self.modes.contains(&mode)
    }

    pub fn remove_mode(&mut self, mode: ExecutionMode) {
        self.modes.retain(|candidate| *candidate != mode);
    }

    pub fn set_mode(&mut self, mode: ExecutionMode, enabled: bool) {
        if enabled {
            self.add_mode(mode);
        } else {
            self.remove_mode(mode);
        }
    }

    pub fn allows_live_transition(&self) -> bool {
        self.transition_policy.allow_live_transition
    }

    pub fn allows_autonomous_transition(&self) -> bool {
        self.transition_policy.autonomous && self.allows_live_transition()
    }

    pub fn transition_mode_allowed(&self, mode: ExecutionMode) -> bool {
        self.has_mode(mode) || self.transition_policy.mode_allowed(mode)
    }

    pub fn normalize(&mut self) {
        let mut seen = HashSet::new();
        self.modes.retain(|mode| seen.insert(*mode));

        let mut related_seen = HashSet::new();
        let mut normalized_related = Vec::new();
        for raw in std::mem::take(&mut self.related_network_ids) {
            if let Some(cleaned) = clean_id(&raw) {
                if related_seen.insert(cleaned.clone()) {
                    normalized_related.push(cleaned);
                }
            }
        }
        self.related_network_ids = normalized_related;

        if let Some(group) = self.combined_group.as_mut() {
            *group = group.trim().to_string();
            if group.is_empty() {
                self.combined_group = None;
            }
        }
        if let Some(group) = self.federation_group.as_mut() {
            *group = group.trim().to_string();
            if group.is_empty() {
                self.federation_group = None;
            }
        }

        if self.has_mode(ExecutionMode::Sharded) {
            self.add_mode(ExecutionMode::Distributed);
        }
        if !self.related_network_ids.is_empty() {
            self.add_mode(ExecutionMode::Grouped);
        }
        if self.combined_group.is_some() {
            self.add_mode(ExecutionMode::Combined);
            self.add_mode(ExecutionMode::Grouped);
        }
        if self.federation_group.is_some() {
            self.add_mode(ExecutionMode::Federated);
            self.add_mode(ExecutionMode::Grouped);
        }
        if matches!(self.scope, ExecutionScope::FederatedCluster) {
            self.add_mode(ExecutionMode::Distributed);
            self.add_mode(ExecutionMode::Federated);
        }

        self.transition_policy.normalize();
    }

    pub fn prefers_sharding(&self) -> bool {
        self.has_mode(ExecutionMode::Sharded)
    }

    pub fn constrains_to_single_target(&self) -> bool {
        self.has_mode(ExecutionMode::Individual)
            || matches!(
                self.scope,
                ExecutionScope::Node | ExecutionScope::Container | ExecutionScope::System
            )
    }

    pub fn requested_shard_count(&self, available_nodes: usize) -> usize {
        if available_nodes == 0 {
            return 0;
        }
        if self.constrains_to_single_target() {
            return 1;
        }
        let desired_shards = self.effective_shard_count(None);
        if desired_shards > 0 {
            return desired_shards.clamp(1, available_nodes);
        }
        available_nodes
    }

    pub fn effective_shard_count(&self, infra: Option<&InfrastructureFingerprint>) -> usize {
        if self.desired_shards > 0 {
            return self.desired_shards;
        }
        infra
            .map(InfrastructureFingerprint::host_count)
            .unwrap_or(0)
    }

    pub fn apply_infrastructure_hint(&mut self, infra: &InfrastructureFingerprint) {
        if self.scope == ExecutionScope::Auto {
            self.scope = if infra.distributed_capable() {
                if self.has_mode(ExecutionMode::Federated) || infra.continuum {
                    ExecutionScope::FederatedCluster
                } else {
                    ExecutionScope::Cluster
                }
            } else if infra.containerized {
                ExecutionScope::Container
            } else if infra.host_count() > 1 {
                ExecutionScope::System
            } else {
                ExecutionScope::Node
            };
        }

        if self.modes.is_empty() && infra.distributed_capable() {
            self.add_mode(ExecutionMode::Distributed);
            if infra.host_count() > 1 || infra.daemonset_workers || infra.openmpi || infra.slurm {
                self.add_mode(ExecutionMode::Sharded);
            }
        }

        if self.max_concurrent_networks == 0 {
            self.max_concurrent_networks = if infra.host_count() > 1 {
                infra.host_count()
            } else {
                std::thread::available_parallelism()
                    .map(|count| count.get())
                    .unwrap_or(1)
            };
        }

        if self.desired_shards == 0 && self.prefers_sharding() {
            self.desired_shards = infra.host_count().max(1);
        }

        self.normalize();
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct InfrastructureFingerprint {
    pub sources: Vec<String>,
    pub host_names: Vec<String>,
    pub kubernetes: bool,
    pub slurm: bool,
    pub openmpi: bool,
    pub containerized: bool,
    pub continuum: bool,
    pub daemonset_workers: bool,
    pub deployment_workers: bool,
    pub singleton_web_ui: bool,
    pub control_plane_pinned: bool,
    pub engine_mode: Option<String>,
    pub orchestrator_service_name: Option<String>,
    pub runtime_root: Option<String>,
    pub startup_network_ids: Vec<String>,
}

impl InfrastructureFingerprint {
    pub fn host_count(&self) -> usize {
        self.host_names.len().max(1)
    }

    pub fn distributed_capable(&self) -> bool {
        self.kubernetes
            || self.slurm
            || self.openmpi
            || self.continuum
            || self.daemonset_workers
            || self.host_names.len() > 1
    }

    pub fn recommended_orchestrator_addr(&self) -> Option<String> {
        self.orchestrator_service_name
            .as_deref()
            .and_then(clean_id)
            .map(|name| format!("http://{}:50051", name))
    }

    fn add_source(&mut self, source: &Path) {
        let value = source.display().to_string();
        if !self.sources.contains(&value) {
            self.sources.push(value);
        }
    }

    fn add_host(&mut self, raw: &str) {
        let Some(host) = clean_id(raw) else {
            return;
        };
        if host.eq_ignore_ascii_case("localhost") || host.eq_ignore_ascii_case("all") {
            return;
        }
        if !self.host_names.contains(&host) {
            self.host_names.push(host);
        }
    }

    fn add_startup_network_id(&mut self, raw: &str) {
        let Some(id) = clean_id(raw) else {
            return;
        };
        if !self.startup_network_ids.contains(&id) {
            self.startup_network_ids.push(id);
        }
    }

    fn set_engine_mode(&mut self, raw: &str) {
        let Some(mode) = clean_id(raw).map(|value| value.to_ascii_lowercase()) else {
            return;
        };
        self.engine_mode = Some(mode.clone());
        self.kubernetes = true;
        match mode.as_str() {
            "daemonset" => self.daemonset_workers = true,
            "deployment" => self.deployment_workers = true,
            _ => {}
        }
    }
}

pub fn default_infrastructure_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(root) = std::env::var("NM_INFRASTRUCTURE_ROOT") {
        let trimmed = root.trim();
        if !trimmed.is_empty() {
            roots.push(PathBuf::from(trimmed));
        }
    }
    if roots.is_empty() {
        let default_root = PathBuf::from(DEFAULT_SWARMHPC_ANSIBLE_ROOT);
        if default_root.exists() {
            roots.push(default_root);
        }
    }
    roots
}

pub fn detect_infrastructure(paths: &[PathBuf]) -> anyhow::Result<InfrastructureFingerprint> {
    let mut fingerprint = detect_live_environment();
    for path in paths {
        if path.exists() {
            scan_path(&mut fingerprint, path)?;
        }
    }
    fingerprint.host_names.sort();
    fingerprint.host_names.dedup();
    fingerprint.sources.sort();
    fingerprint.sources.dedup();
    fingerprint.startup_network_ids.sort();
    fingerprint.startup_network_ids.dedup();
    Ok(fingerprint)
}

fn detect_live_environment() -> InfrastructureFingerprint {
    let mut fingerprint = InfrastructureFingerprint::default();
    if std::env::var_os("KUBERNETES_SERVICE_HOST").is_some() {
        fingerprint.kubernetes = true;
        fingerprint.containerized = true;
    }
    if std::env::var_os("SLURM_JOB_ID").is_some() {
        fingerprint.slurm = true;
    }
    if std::env::var_os("OMPI_COMM_WORLD_SIZE").is_some()
        || std::env::var_os("MPI_LOCALNRANKS").is_some()
    {
        fingerprint.openmpi = true;
    }
    if std::env::var_os("NM_RUNTIME_CONTINUUM_URL").is_some()
        || std::env::var_os("NM_RUNTIME_CONTINUUM_HOSTS").is_some()
    {
        fingerprint.continuum = true;
    }
    if std::env::var_os("container").is_some()
        || std::env::var_os("CONTAINER").is_some()
        || Path::new("/.dockerenv").exists()
    {
        fingerprint.containerized = true;
    }
    fingerprint
}

fn scan_path(fingerprint: &mut InfrastructureFingerprint, path: &Path) -> anyhow::Result<()> {
    if path.is_file() {
        scan_file(fingerprint, path)?;
        return Ok(());
    }

    for entry in std::fs::read_dir(path)
        .with_context(|| format!("failed to read infrastructure path '{}'", path.display()))?
    {
        let entry = entry.with_context(|| {
            format!(
                "failed to read infrastructure entry in '{}'",
                path.display()
            )
        })?;
        let entry_path = entry.path();
        if entry_path.is_dir() {
            scan_path(fingerprint, &entry_path)?;
        } else if entry_path.is_file() {
            scan_file(fingerprint, &entry_path)?;
        }
    }
    Ok(())
}

fn scan_file(fingerprint: &mut InfrastructureFingerprint, path: &Path) -> anyhow::Result<()> {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .unwrap_or_default();
    if !matches!(extension.as_str(), "yml" | "yaml" | "j2" | "ini" | "cfg") {
        return Ok(());
    }

    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read infrastructure file '{}'", path.display()))?;
    fingerprint.add_source(path);

    if matches!(extension.as_str(), "ini" | "cfg") {
        scan_inventory_text(fingerprint, &text);
    }
    scan_structured_text(fingerprint, &text);
    Ok(())
}

fn scan_inventory_text(fingerprint: &mut InfrastructureFingerprint, text: &str) {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.starts_with(';')
            || trimmed.starts_with('[')
        {
            continue;
        }
        let first = trimmed.split_whitespace().next().unwrap_or_default();
        if first.contains('=') {
            continue;
        }
        fingerprint.add_host(first);
    }
}

fn parse_yaml_scalar(line: &str, key: &str) -> Option<String> {
    let trimmed = line.trim();
    let (left, right) = trimmed.split_once(':')?;
    if left.trim() != key {
        return None;
    }
    let without_comment = right.split('#').next().unwrap_or_default().trim();
    clean_id(without_comment)
}

fn scan_structured_text(fingerprint: &mut InfrastructureFingerprint, text: &str) {
    let lower = text.to_ascii_lowercase();
    if lower.contains("kubernetes")
        || lower.contains("kubeconfig")
        || lower.contains("kind: deployment")
        || lower.contains("kind: daemonset")
        || lower.contains("k3s")
        || lower.contains("kubectl")
    {
        fingerprint.kubernetes = true;
    }
    if lower.contains("slurm") {
        fingerprint.slurm = true;
    }
    if lower.contains("mpirun") || lower.contains("openmpi") || lower.contains("ompi_comm_world") {
        fingerprint.openmpi = true;
    }
    if lower.contains("podman")
        || lower.contains("docker")
        || lower.contains("container")
        || lower.contains("imagepullsecrets")
    {
        fingerprint.containerized = true;
    }
    if lower.contains("continuum_") || lower.contains("nm_runtime_continuum_") {
        fingerprint.continuum = true;
    }
    if lower.contains("web_ui_require_singleton: true")
        || lower.contains("nmchain_require_singleton_web_ui")
        || lower.contains("singleton")
    {
        fingerprint.singleton_web_ui = true;
    }
    if lower.contains("place_web_ui_on_control_plane: true")
        || lower.contains("orchestrator_node_name:")
        || lower.contains("node-role.kubernetes.io/control-plane")
    {
        fingerprint.control_plane_pinned = true;
    }
    if lower.contains("kind: daemonset") {
        fingerprint.daemonset_workers = true;
        fingerprint.engine_mode = Some("daemonset".to_string());
    }

    for line in text.lines() {
        if let Some(host) = parse_yaml_scalar(line, "hosts") {
            fingerprint.add_host(&host);
        }
        if let Some(mode) = parse_yaml_scalar(line, "continuum_tenant_aarnn_engine_mode") {
            fingerprint.set_engine_mode(&mode);
        }
        if let Some(service) =
            parse_yaml_scalar(line, "continuum_tenant_aarnn_orchestrator_service_name")
        {
            fingerprint.orchestrator_service_name = Some(service);
        }
        if let Some(runtime_root) = parse_yaml_scalar(line, "continuum_tenant_aarnn_runtime_root") {
            fingerprint.runtime_root = Some(runtime_root);
        }
        if let Some(network_id) =
            parse_yaml_scalar(line, "continuum_tenant_aarnn_startup_network_id")
        {
            fingerprint.add_startup_network_id(&network_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "aarnn-deployment-test-{}-{}",
            label,
            fastrand::u32(..)
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn deployment_groups_are_normalized() {
        let mut cfg = DeploymentConfig {
            modes: vec![ExecutionMode::Federated, ExecutionMode::Federated],
            combined_group: Some("combo-a".to_string()),
            federation_group: Some("fed-a".to_string()),
            related_network_ids: vec!["alpha".to_string(), "alpha".to_string()],
            ..DeploymentConfig::default()
        };
        cfg.normalize();
        assert!(cfg.has_mode(ExecutionMode::Combined));
        assert!(cfg.has_mode(ExecutionMode::Grouped));
        assert!(cfg.has_mode(ExecutionMode::Federated));
        assert_eq!(cfg.related_network_ids, vec!["alpha".to_string()]);
    }

    #[test]
    fn swarmhpc_like_ansible_is_detected() {
        let root = temp_dir("swarmhpc");
        let roles = root.join("roles/continuum_tenant_aarnn/defaults");
        std::fs::create_dir_all(&roles).unwrap();
        std::fs::write(
            root.join("continuum_tenant_aarnn_site.yml"),
            r#"---
- name: Deploy AARNN
  hosts: rk1
"#,
        )
        .unwrap();
        std::fs::write(
            roles.join("main.yml"),
            r#"continuum_tenant_aarnn_engine_mode: "daemonset"
continuum_tenant_aarnn_orchestrator_service_name: "aarnn-orchestrator"
continuum_tenant_aarnn_runtime_root: "/var/lib/aarnn/runtime"
continuum_tenant_aarnn_startup_network_id: "tenant-aarnn"
continuum_tenant_aarnn_continuum_url: "https://continuum.example"
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("inventory.ini"),
            "qc01 ansible_host=192.168.1.60\nrk1 ansible_host=192.168.1.2\n",
        )
        .unwrap();

        let fingerprint = detect_infrastructure(&[root]).unwrap();
        assert!(fingerprint.kubernetes);
        assert!(fingerprint.daemonset_workers);
        assert!(fingerprint.continuum);
        assert_eq!(
            fingerprint.recommended_orchestrator_addr().as_deref(),
            Some("http://aarnn-orchestrator:50051")
        );
        assert_eq!(
            fingerprint.runtime_root.as_deref(),
            Some("/var/lib/aarnn/runtime")
        );
        assert!(
            fingerprint
                .startup_network_ids
                .contains(&"tenant-aarnn".to_string())
        );
        assert!(fingerprint.host_names.contains(&"qc01".to_string()));
        assert!(fingerprint.host_names.contains(&"rk1".to_string()));
    }

    #[test]
    fn infrastructure_hint_enables_cluster_modes() {
        let mut cfg = DeploymentConfig {
            autodetect_infrastructure: true,
            ..DeploymentConfig::default()
        };
        let fingerprint = InfrastructureFingerprint {
            kubernetes: true,
            daemonset_workers: true,
            continuum: true,
            host_names: vec!["qc01".to_string(), "rk1".to_string()],
            ..InfrastructureFingerprint::default()
        };
        cfg.apply_infrastructure_hint(&fingerprint);
        assert!(cfg.has_mode(ExecutionMode::Distributed));
        assert!(cfg.has_mode(ExecutionMode::Sharded));
        assert_eq!(cfg.scope, ExecutionScope::FederatedCluster);
        assert_eq!(cfg.max_concurrent_networks, 2);
        assert_eq!(cfg.desired_shards, 2);
    }

    #[test]
    fn node_scope_constrains_sharding_to_one_target() {
        let cfg = DeploymentConfig {
            modes: vec![ExecutionMode::Distributed, ExecutionMode::Sharded],
            scope: ExecutionScope::Node,
            desired_shards: 4,
            ..DeploymentConfig::default()
        };

        assert!(cfg.prefers_sharding());
        assert!(cfg.constrains_to_single_target());
        assert_eq!(cfg.requested_shard_count(6), 1);
    }

    #[test]
    fn autonomous_transition_policy_enables_live_transition() {
        let mut cfg = DeploymentConfig {
            transition_policy: DeploymentTransitionPolicy {
                autonomous: true,
                permitted_modes: vec![ExecutionMode::Sharded, ExecutionMode::Sharded],
                ..DeploymentTransitionPolicy::default()
            },
            ..DeploymentConfig::default()
        };

        cfg.normalize();

        assert!(cfg.allows_live_transition());
        assert!(cfg.allows_autonomous_transition());
        assert_eq!(
            cfg.transition_policy.permitted_modes,
            vec![ExecutionMode::Sharded]
        );
    }
}
