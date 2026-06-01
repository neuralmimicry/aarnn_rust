use crate::distributed::proto::{
    StatusRequest, distributed_neuromorphic_client::DistributedNeuromorphicClient,
};
use crate::engine::{EnginePayloadKind, EngineSpec, RunnerEngine};
use crate::runtime_api::{
    AutoscalerReport, RuntimeStatusResponse, WorkspaceActivityResponse, WorkspaceControlAction,
    WorkspaceCreateRequest, WorkspaceDetailResponse, WorkspaceImportRequest,
    WorkspaceSnapshotResponse, WorkspaceSummary,
};
use crate::shared_fs::{FileLease, acquire_lease_with_timeout, try_acquire_lease};
use anyhow::{Context, anyhow};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{Mutex as AsyncMutex, RwLock, Semaphore, watch};
use tokio::time::{Duration, MissedTickBehavior};
use tonic::Request;

const MANIFEST_FILE: &str = "manifest.json";
const BASELINE_SNAPSHOT_FILE: &str = "baseline.snapshot.json";
const LATEST_SNAPSHOT_FILE: &str = "latest.snapshot.json";
const WORKSPACE_LEASE_FILE: &str = ".runtime-lease.lock";
const COORDINATION_DIR: &str = "_coordination";
const AUTOSCALER_LOCK_FILE: &str = "continuum-autoscaler.lock";
const AUTOSCALER_STATE_FILE: &str = "continuum-autoscaler.json";

fn default_root_dir() -> PathBuf {
    PathBuf::from("data/runtime")
}

fn default_worker_limit() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .max(1)
}

fn default_resume_existing_workspaces() -> bool {
    true
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn div_ceil(lhs: usize, rhs: usize) -> usize {
    if rhs == 0 {
        return 0;
    }
    lhs.saturating_add(rhs - 1) / rhs
}

fn sanitize_segment(raw: &str, fallback: &str) -> String {
    let cleaned: String = raw
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '-'
            }
        })
        .collect();
    let normalized = cleaned
        .trim_matches(|ch| ch == '-' || ch == '.')
        .to_lowercase();
    if normalized.is_empty() {
        fallback.to_string()
    } else {
        normalized
    }
}

fn normalize_grpc_addr(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("http://{}", trimmed)
    }
}

fn generate_workspace_id(name: Option<&str>) -> String {
    let base = name
        .map(|value| sanitize_segment(value, "workspace"))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "workspace".to_string());
    format!("{}-{:08x}", base, fastrand::u32(..))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create parent dir '{}'", parent.display()))?;
    }
    let tmp_path = path.with_extension(format!("tmp-{}", fastrand::u32(..)));
    std::fs::write(&tmp_path, bytes)
        .with_context(|| format!("failed to write temp file '{}'", tmp_path.display()))?;
    std::fs::rename(&tmp_path, path).with_context(|| {
        format!(
            "failed to rename temp file '{}' to '{}'",
            tmp_path.display(),
            path.display()
        )
    })?;
    Ok(())
}

fn read_if_exists(path: &Path) -> anyhow::Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    let data = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read '{}'", path.display()))?;
    Ok(Some(data))
}

fn read_dir_paths(root: &Path, label: &str) -> anyhow::Result<Vec<PathBuf>> {
    let entries =
        std::fs::read_dir(root).with_context(|| format!("failed to scan '{}'", root.display()))?;
    let mut paths = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => paths.push(entry.path()),
            Err(err) => {
                nm_err!(
                    "[warn] failed reading {} dir entry from '{}': {}",
                    label,
                    root.display(),
                    err
                );
            }
        }
    }
    Ok(paths)
}

fn file_modified_ms(path: &Path) -> anyhow::Result<Option<u64>> {
    if !path.exists() {
        return Ok(None);
    }
    let modified = std::fs::metadata(path)
        .with_context(|| format!("failed to stat '{}'", path.display()))?
        .modified()
        .with_context(|| format!("failed to read mtime for '{}'", path.display()))?;
    Ok(Some(
        modified
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    ))
}

#[derive(Clone, Debug)]
pub struct RuntimeMetrics {
    pub total_users: usize,
    pub total_workspaces: usize,
    pub running_workspaces: usize,
    pub local_worker_limit: usize,
    pub local_cpu_usage_pct: f32,
    pub local_memory_usage_pct: f32,
    pub local_avg_step_time_ms: f32,
    pub demand_ratio: f32,
}

#[derive(Clone, Debug, Default)]
struct ClusterTelemetry {
    nodes: usize,
    avg_cpu_usage_pct: f32,
    avg_step_time_ms: f32,
    load_skew: f32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct ContinuumAutoscalerState {
    #[serde(default)]
    recruited_hosts: Vec<String>,
}

type AutoscalerFuture<'a> =
    Pin<Box<dyn Future<Output = anyhow::Result<AutoscalerReport>> + Send + 'a>>;

pub trait RuntimeAutoscaler: Send + Sync {
    fn evaluate<'a>(&'a self, metrics: RuntimeMetrics) -> AutoscalerFuture<'a>;
}

#[derive(Debug)]
struct NoopAutoscaler {
    local_worker_limit: usize,
}

impl RuntimeAutoscaler for NoopAutoscaler {
    fn evaluate<'a>(&'a self, _metrics: RuntimeMetrics) -> AutoscalerFuture<'a> {
        Box::pin(async move {
            Ok(AutoscalerReport {
                provider: "local".to_string(),
                enabled: false,
                local_worker_limit: self.local_worker_limit,
                requested_remote_nodes: 0,
                active_remote_nodes: 0,
                last_action: None,
                controller_role: None,
                pressure_signals: Vec::new(),
                local_cpu_usage_pct: None,
                local_memory_usage_pct: None,
                local_avg_step_time_ms: None,
                cluster_nodes: None,
                cluster_avg_cpu_usage_pct: None,
                cluster_avg_step_time_ms: None,
                cluster_load_skew: None,
            })
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContinuumAutoscalerConfig {
    pub base_url: String,
    #[serde(default)]
    pub bearer_token: Option<String>,
    pub recruit_hosts: Vec<String>,
    #[serde(default = "default_recruit_user")]
    pub recruit_user: String,
    #[serde(default)]
    pub ssh_key_path: Option<String>,
    #[serde(default = "default_node_type")]
    pub node_type: String,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub tenant_id: Option<String>,
    #[serde(default)]
    pub tenant_name: Option<String>,
    #[serde(default)]
    pub tenant_environment: Option<String>,
    #[serde(default)]
    pub recruit_token: Option<String>,
    #[serde(default = "default_worker_capacity_per_node")]
    pub worker_capacity_per_node: usize,
    #[serde(default = "default_auto_configure")]
    pub auto_configure: bool,
    #[serde(default = "default_dry_run")]
    pub dry_run: bool,
    #[serde(default = "default_scale_out_load_ratio")]
    pub scale_out_load_ratio: f32,
    #[serde(default = "default_scale_out_cpu_usage_pct")]
    pub scale_out_cpu_usage_pct: f32,
    #[serde(default = "default_scale_out_memory_usage_pct")]
    pub scale_out_memory_usage_pct: f32,
    #[serde(default = "default_scale_out_step_time_ms")]
    pub scale_out_step_time_ms: f32,
    #[serde(default = "default_cluster_cpu_usage_pct")]
    pub cluster_cpu_usage_pct: f32,
    #[serde(default = "default_cluster_step_time_ms")]
    pub cluster_step_time_ms: f32,
    #[serde(default = "default_cluster_load_skew")]
    pub cluster_load_skew: f32,
    #[serde(default = "default_max_recruits_per_tick")]
    pub max_recruits_per_tick: usize,
}

fn default_recruit_user() -> String {
    "ubuntu".to_string()
}

fn default_node_type() -> String {
    "bare-metal".to_string()
}

fn default_worker_capacity_per_node() -> usize {
    4
}

fn default_auto_configure() -> bool {
    false
}

fn default_dry_run() -> bool {
    true
}

fn default_scale_out_load_ratio() -> f32 {
    1.0
}

fn default_scale_out_cpu_usage_pct() -> f32 {
    75.0
}

fn default_scale_out_memory_usage_pct() -> f32 {
    78.0
}

fn default_scale_out_step_time_ms() -> f32 {
    18.0
}

fn default_cluster_cpu_usage_pct() -> f32 {
    70.0
}

fn default_cluster_step_time_ms() -> f32 {
    16.0
}

fn default_cluster_load_skew() -> f32 {
    1.4
}

fn default_max_recruits_per_tick() -> usize {
    1
}

impl ContinuumAutoscalerConfig {
    pub fn from_env() -> Option<Self> {
        let base_url = std::env::var("NM_RUNTIME_CONTINUUM_URL").ok()?;
        let hosts_raw = std::env::var("NM_RUNTIME_CONTINUUM_HOSTS").ok()?;
        let recruit_hosts: Vec<String> = hosts_raw
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        if recruit_hosts.is_empty() {
            return None;
        }
        Some(Self {
            base_url,
            bearer_token: std::env::var("NM_RUNTIME_CONTINUUM_TOKEN").ok(),
            recruit_hosts,
            recruit_user: std::env::var("NM_RUNTIME_CONTINUUM_USER")
                .unwrap_or_else(|_| default_recruit_user()),
            ssh_key_path: std::env::var("NM_RUNTIME_CONTINUUM_SSH_KEY").ok(),
            node_type: std::env::var("NM_RUNTIME_CONTINUUM_NODE_TYPE")
                .unwrap_or_else(|_| default_node_type()),
            region: std::env::var("NM_RUNTIME_CONTINUUM_REGION").ok(),
            tenant_id: std::env::var("NM_RUNTIME_CONTINUUM_TENANT_ID").ok(),
            tenant_name: std::env::var("NM_RUNTIME_CONTINUUM_TENANT_NAME").ok(),
            tenant_environment: std::env::var("NM_RUNTIME_CONTINUUM_TENANT_ENV").ok(),
            recruit_token: std::env::var("NM_RUNTIME_CONTINUUM_RECRUIT_TOKEN").ok(),
            worker_capacity_per_node: std::env::var("NM_RUNTIME_CONTINUUM_WORKERS_PER_NODE")
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .filter(|value| *value > 0)
                .unwrap_or_else(default_worker_capacity_per_node),
            auto_configure: std::env::var("NM_RUNTIME_CONTINUUM_AUTO_CONFIGURE")
                .ok()
                .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "on"))
                .unwrap_or_else(default_auto_configure),
            dry_run: std::env::var("NM_RUNTIME_CONTINUUM_DRY_RUN")
                .ok()
                .map(|value| !matches!(value.as_str(), "0" | "false" | "FALSE" | "no" | "off"))
                .unwrap_or_else(default_dry_run),
            scale_out_load_ratio: std::env::var("NM_RUNTIME_CONTINUUM_SCALE_OUT_LOAD_RATIO")
                .ok()
                .and_then(|value| value.parse::<f32>().ok())
                .filter(|value| value.is_finite() && *value > 0.0)
                .unwrap_or_else(default_scale_out_load_ratio),
            scale_out_cpu_usage_pct: std::env::var("NM_RUNTIME_CONTINUUM_SCALE_OUT_CPU_PCT")
                .ok()
                .and_then(|value| value.parse::<f32>().ok())
                .filter(|value| value.is_finite() && *value > 0.0)
                .unwrap_or_else(default_scale_out_cpu_usage_pct),
            scale_out_memory_usage_pct: std::env::var("NM_RUNTIME_CONTINUUM_SCALE_OUT_MEMORY_PCT")
                .ok()
                .and_then(|value| value.parse::<f32>().ok())
                .filter(|value| value.is_finite() && *value > 0.0)
                .unwrap_or_else(default_scale_out_memory_usage_pct),
            scale_out_step_time_ms: std::env::var("NM_RUNTIME_CONTINUUM_SCALE_OUT_STEP_MS")
                .ok()
                .and_then(|value| value.parse::<f32>().ok())
                .filter(|value| value.is_finite() && *value > 0.0)
                .unwrap_or_else(default_scale_out_step_time_ms),
            cluster_cpu_usage_pct: std::env::var("NM_RUNTIME_CONTINUUM_CLUSTER_CPU_PCT")
                .ok()
                .and_then(|value| value.parse::<f32>().ok())
                .filter(|value| value.is_finite() && *value > 0.0)
                .unwrap_or_else(default_cluster_cpu_usage_pct),
            cluster_step_time_ms: std::env::var("NM_RUNTIME_CONTINUUM_CLUSTER_STEP_MS")
                .ok()
                .and_then(|value| value.parse::<f32>().ok())
                .filter(|value| value.is_finite() && *value > 0.0)
                .unwrap_or_else(default_cluster_step_time_ms),
            cluster_load_skew: std::env::var("NM_RUNTIME_CONTINUUM_CLUSTER_LOAD_SKEW")
                .ok()
                .and_then(|value| value.parse::<f32>().ok())
                .filter(|value| value.is_finite() && *value > 0.0)
                .unwrap_or_else(default_cluster_load_skew),
            max_recruits_per_tick: std::env::var("NM_RUNTIME_CONTINUUM_MAX_RECRUITS_PER_TICK")
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .filter(|value| *value > 0)
                .unwrap_or_else(default_max_recruits_per_tick),
        })
    }
}

#[derive(Debug)]
struct ContinuumAutoscaler {
    config: ContinuumAutoscalerConfig,
    local_worker_limit: usize,
    orchestrator_addr: Option<String>,
    coordination_dir: PathBuf,
    client: reqwest::Client,
    last_action: RwLock<Option<String>>,
}

impl ContinuumAutoscaler {
    fn new(
        config: ContinuumAutoscalerConfig,
        local_worker_limit: usize,
        root_dir: &Path,
        orchestrator_addr: Option<String>,
    ) -> Self {
        Self {
            config,
            local_worker_limit,
            orchestrator_addr,
            coordination_dir: root_dir.join(COORDINATION_DIR),
            client: reqwest::Client::new(),
            last_action: RwLock::new(None),
        }
    }

    fn controller_lock_path(&self) -> PathBuf {
        self.coordination_dir.join(AUTOSCALER_LOCK_FILE)
    }

    fn state_path(&self) -> PathBuf {
        self.coordination_dir.join(AUTOSCALER_STATE_FILE)
    }

    fn load_state(&self) -> anyhow::Result<ContinuumAutoscalerState> {
        let Some(raw) = read_if_exists(&self.state_path())? else {
            return Ok(ContinuumAutoscalerState::default());
        };
        serde_json::from_str(&raw).context("failed to parse autoscaler state")
    }

    fn save_state(&self, state: &ContinuumAutoscalerState) -> anyhow::Result<()> {
        let bytes =
            serde_json::to_vec_pretty(state).context("failed to encode autoscaler state")?;
        atomic_write(&self.state_path(), &bytes)
    }

    async fn recruit_host(&self, host: &str) -> anyhow::Result<()> {
        let url = format!(
            "{}/node/recruit",
            self.config.base_url.trim_end_matches('/')
        );
        let mut request = self
            .client
            .post(url)
            .header(CONTENT_TYPE, "application/json");
        if let Some(token) = self.config.bearer_token.as_deref() {
            request = request.header(AUTHORIZATION, format!("Bearer {}", token));
        }

        let mut body = serde_json::json!({
            "host": host,
            "user": self.config.recruit_user,
            "node_type": self.config.node_type,
            "name": format!("aarnn-{}", sanitize_segment(host, "remote-node")),
            "dry_run": self.config.dry_run,
            "auto_configure": self.config.auto_configure,
        });

        if let Some(path) = self.config.ssh_key_path.as_deref() {
            body["ssh_key_path"] = serde_json::Value::String(path.to_string());
        }
        if let Some(region) = self.config.region.as_deref() {
            body["region"] = serde_json::Value::String(region.to_string());
        }
        if let Some(tenant_id) = self.config.tenant_id.as_deref() {
            body["tenant_id"] = serde_json::Value::String(tenant_id.to_string());
        }
        if let Some(tenant_name) = self.config.tenant_name.as_deref() {
            body["tenant_name"] = serde_json::Value::String(tenant_name.to_string());
        }
        if let Some(tenant_environment) = self.config.tenant_environment.as_deref() {
            body["tenant_environment"] = serde_json::Value::String(tenant_environment.to_string());
        }
        if let Some(recruit_token) = self.config.recruit_token.as_deref() {
            body["recruit_token"] = serde_json::Value::String(recruit_token.to_string());
        }

        let response = request
            .json(&body)
            .send()
            .await
            .with_context(|| format!("failed to recruit Continuum host '{}'", host))?;
        if !response.status().is_success() {
            let text = response
                .text()
                .await
                .unwrap_or_else(|_| "recruit failed".to_string());
            return Err(anyhow!("Continuum recruit failed for '{}': {}", host, text));
        }
        Ok(())
    }

    async fn fetch_cluster_telemetry(&self) -> anyhow::Result<Option<ClusterTelemetry>> {
        let Some(addr) = self
            .orchestrator_addr
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        else {
            return Ok(None);
        };

        let mut client = DistributedNeuromorphicClient::connect(normalize_grpc_addr(addr))
            .await
            .context("failed to connect to orchestrator for autoscaler telemetry")?;
        let status = client
            .get_system_status(Request::new(StatusRequest {}))
            .await
            .context("failed to query orchestrator status")?
            .into_inner();
        if status.nodes.is_empty() {
            return Ok(None);
        }

        let mut cpu_sum = 0.0f32;
        let mut step_sum = 0.0f32;
        let mut counted = 0usize;
        let mut load_scores = Vec::new();

        for node in status.nodes {
            if let Some(resources) = node.resources {
                counted += 1;
                cpu_sum += resources.cpu_usage.max(0.0);
                step_sum += resources.avg_step_time_ms.max(0.0);
                let capacity = resources.capacity_score.max(0.1);
                let cpu_load = (resources.cpu_usage / 100.0).clamp(0.0, 4.0);
                let network_load = node.active_networks.len() as f32 / capacity;
                load_scores.push(cpu_load.max(network_load).max(0.01));
            }
        }

        if counted == 0 {
            return Ok(None);
        }

        let max_load = load_scores.iter().copied().fold(0.0f32, f32::max);
        let min_load = load_scores.iter().copied().fold(f32::MAX, f32::min);
        let load_skew = if max_load <= 0.0 {
            0.0
        } else {
            max_load / min_load.max(0.05)
        };

        Ok(Some(ClusterTelemetry {
            nodes: counted,
            avg_cpu_usage_pct: cpu_sum / counted as f32,
            avg_step_time_ms: step_sum / counted as f32,
            load_skew,
        }))
    }

    fn build_report(
        &self,
        controller_role: &str,
        state: &ContinuumAutoscalerState,
        metrics: &RuntimeMetrics,
        cluster: Option<&ClusterTelemetry>,
        requested_remote_nodes: usize,
        last_action: Option<String>,
        pressure_signals: Vec<String>,
    ) -> AutoscalerReport {
        AutoscalerReport {
            provider: "continuum".to_string(),
            enabled: true,
            local_worker_limit: self.local_worker_limit,
            requested_remote_nodes,
            active_remote_nodes: state.recruited_hosts.len().max(
                cluster
                    .map(|value| value.nodes.saturating_sub(1))
                    .unwrap_or(0),
            ),
            last_action,
            controller_role: Some(controller_role.to_string()),
            pressure_signals,
            local_cpu_usage_pct: Some(metrics.local_cpu_usage_pct),
            local_memory_usage_pct: Some(metrics.local_memory_usage_pct),
            local_avg_step_time_ms: Some(metrics.local_avg_step_time_ms),
            cluster_nodes: cluster.map(|value| value.nodes),
            cluster_avg_cpu_usage_pct: cluster.map(|value| value.avg_cpu_usage_pct),
            cluster_avg_step_time_ms: cluster.map(|value| value.avg_step_time_ms),
            cluster_load_skew: cluster.map(|value| value.load_skew),
        }
    }
}

impl RuntimeAutoscaler for ContinuumAutoscaler {
    fn evaluate<'a>(&'a self, metrics: RuntimeMetrics) -> AutoscalerFuture<'a> {
        Box::pin(async move {
            let controller_lock_path = self.controller_lock_path();
            let controller_lock =
                tokio::task::spawn_blocking(move || -> anyhow::Result<Option<FileLease>> {
                    try_acquire_lease(&controller_lock_path)
                })
                .await
                .context("autoscaler controller lease task failed")??;

            let cluster = match self.fetch_cluster_telemetry().await {
                Ok(cluster) => cluster,
                Err(err) => {
                    let mut last_action = self.last_action.write().await;
                    *last_action = Some(format!("cluster telemetry unavailable: {}", err));
                    None
                }
            };

            let mut state = match self.load_state() {
                Ok(state) => state,
                Err(err) => {
                    let mut last_action = self.last_action.write().await;
                    *last_action = Some(format!("failed loading autoscaler state: {}", err));
                    ContinuumAutoscalerState::default()
                }
            };

            let demand_nodes = div_ceil(
                metrics
                    .running_workspaces
                    .saturating_sub(metrics.local_worker_limit),
                self.config.worker_capacity_per_node.max(1),
            );
            let mut pressure_signals = Vec::new();
            if metrics.demand_ratio >= self.config.scale_out_load_ratio {
                pressure_signals.push(format!(
                    "local load ratio {:.2} >= {:.2}",
                    metrics.demand_ratio, self.config.scale_out_load_ratio
                ));
            }
            if metrics.local_cpu_usage_pct >= self.config.scale_out_cpu_usage_pct {
                pressure_signals.push(format!(
                    "local cpu {:.1}% >= {:.1}%",
                    metrics.local_cpu_usage_pct, self.config.scale_out_cpu_usage_pct
                ));
            }
            if metrics.local_memory_usage_pct >= self.config.scale_out_memory_usage_pct {
                pressure_signals.push(format!(
                    "local memory {:.1}% >= {:.1}%",
                    metrics.local_memory_usage_pct, self.config.scale_out_memory_usage_pct
                ));
            }
            if metrics.local_avg_step_time_ms >= self.config.scale_out_step_time_ms {
                pressure_signals.push(format!(
                    "local step {:.2}ms >= {:.2}ms",
                    metrics.local_avg_step_time_ms, self.config.scale_out_step_time_ms
                ));
            }
            if let Some(cluster) = cluster.as_ref() {
                if cluster.avg_cpu_usage_pct >= self.config.cluster_cpu_usage_pct {
                    pressure_signals.push(format!(
                        "cluster cpu {:.1}% >= {:.1}%",
                        cluster.avg_cpu_usage_pct, self.config.cluster_cpu_usage_pct
                    ));
                }
                if cluster.avg_step_time_ms >= self.config.cluster_step_time_ms {
                    pressure_signals.push(format!(
                        "cluster step {:.2}ms >= {:.2}ms",
                        cluster.avg_step_time_ms, self.config.cluster_step_time_ms
                    ));
                }
                if cluster.load_skew >= self.config.cluster_load_skew {
                    pressure_signals.push(format!(
                        "cluster load skew {:.2} >= {:.2}",
                        cluster.load_skew, self.config.cluster_load_skew
                    ));
                }
            }

            let telemetry_nodes = if pressure_signals.is_empty() {
                0
            } else {
                1 + pressure_signals.len().saturating_sub(1) / 2
            };
            let desired_remote_nodes = demand_nodes
                .max(telemetry_nodes)
                .min(self.config.recruit_hosts.len());
            let controller_role = if controller_lock.is_some() {
                "controller"
            } else {
                "standby"
            };

            if controller_lock.is_none() {
                let last_action = self.last_action.read().await.clone().or_else(|| {
                    Some(
                        "standby: autoscaler controller lease held by another runtime pod"
                            .to_string(),
                    )
                });
                return Ok(self.build_report(
                    controller_role,
                    &state,
                    &metrics,
                    cluster.as_ref(),
                    desired_remote_nodes,
                    last_action,
                    pressure_signals,
                ));
            }
            let _controller_lock = controller_lock;

            let mut last_action = self.last_action.write().await;
            let mut recruited_this_tick = 0usize;
            while state.recruited_hosts.len() < desired_remote_nodes
                && recruited_this_tick < self.config.max_recruits_per_tick.max(1)
            {
                let Some(next_host) = self
                    .config
                    .recruit_hosts
                    .iter()
                    .find(|host| !state.recruited_hosts.iter().any(|entry| entry == *host))
                    .cloned()
                else {
                    *last_action = Some(format!(
                        "remote capacity requested for {} extra nodes, but no unused Continuum hosts remain",
                        desired_remote_nodes.saturating_sub(state.recruited_hosts.len())
                    ));
                    break;
                };

                match self.recruit_host(&next_host).await {
                    Ok(_) => {
                        state.recruited_hosts.push(next_host.clone());
                        state.recruited_hosts.sort();
                        state.recruited_hosts.dedup();
                        self.save_state(&state)?;
                        recruited_this_tick += 1;
                        let reason = if pressure_signals.is_empty() {
                            format!(
                                "local demand {} running workspaces over {} local workers",
                                metrics.running_workspaces, metrics.local_worker_limit
                            )
                        } else {
                            pressure_signals.join(", ")
                        };
                        *last_action = Some(format!(
                            "recruited Continuum host '{}' due to {}",
                            next_host, reason
                        ));
                    }
                    Err(err) => {
                        *last_action = Some(format!(
                            "failed recruiting Continuum host '{}': {}",
                            next_host, err
                        ));
                        break;
                    }
                }
            }

            Ok(self.build_report(
                controller_role,
                &state,
                &metrics,
                cluster.as_ref(),
                desired_remote_nodes,
                last_action.clone(),
                pressure_signals,
            ))
        })
    }
}

#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    pub root_dir: PathBuf,
    pub tick_interval_ms: u64,
    pub local_worker_limit: usize,
    pub resume_existing_workspaces: bool,
    pub autosave_steps: u64,
    pub continuum: Option<ContinuumAutoscalerConfig>,
    pub reconcile_interval_ms: u64,
    pub autoscaler_interval_ms: u64,
    pub orchestrator_addr: Option<String>,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            root_dir: default_root_dir(),
            tick_interval_ms: 25,
            local_worker_limit: default_worker_limit(),
            resume_existing_workspaces: default_resume_existing_workspaces(),
            autosave_steps: 50,
            continuum: ContinuumAutoscalerConfig::from_env(),
            reconcile_interval_ms: 1000,
            autoscaler_interval_ms: 2000,
            orchestrator_addr: None,
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct WorkspaceKey {
    user_id: String,
    workspace_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct WorkspaceManifest {
    version: u32,
    user_id: String,
    workspace_id: String,
    name: String,
    created_at_ms: u64,
    updated_at_ms: u64,
    last_saved_at_ms: Option<u64>,
    desired_running: bool,
    autosave_steps: u64,
    engine: EngineSpec,
}

impl WorkspaceManifest {
    fn summary(&self, status: &crate::engine::EngineStatus, running: bool) -> WorkspaceSummary {
        WorkspaceSummary {
            owner_id: self.user_id.clone(),
            workspace_id: self.workspace_id.clone(),
            network_id: self.workspace_id.clone(),
            name: self.name.clone(),
            running,
            step: status.step,
            sim_time_ms: status.sim_time_ms,
            num_sensory_neurons: status.num_sensory_neurons,
            num_hidden_layers: status.num_hidden_layers,
            num_output_neurons: status.num_output_neurons,
            total_neurons: status.total_neurons,
            desired_aarnn_depth: status.desired_aarnn_depth,
            created_at_ms: self.created_at_ms,
            updated_at_ms: self.updated_at_ms,
            last_saved_at_ms: self.last_saved_at_ms,
            distributed_node_count: 0,
            distributed_node_ids: Vec::new(),
        }
    }
}

struct WorkspaceHandle {
    key: WorkspaceKey,
    dir: PathBuf,
    manifest: RwLock<WorkspaceManifest>,
    engine: Mutex<RunnerEngine>,
    running: AtomicBool,
    resume_suppressed: AtomicBool,
    stepping: AtomicBool,
    avg_step_time_micros: AtomicU64,
    status_cache: RwLock<crate::engine::EngineStatus>,
    activity_cache: RwLock<crate::engine::EngineActivity>,
}

impl WorkspaceHandle {
    fn manifest_path(&self) -> PathBuf {
        self.dir.join(MANIFEST_FILE)
    }

    fn baseline_snapshot_path(&self) -> PathBuf {
        self.dir.join(BASELINE_SNAPSHOT_FILE)
    }

    fn latest_snapshot_path(&self) -> PathBuf {
        self.dir.join(LATEST_SNAPSHOT_FILE)
    }

    fn lease_path(&self) -> PathBuf {
        self.dir.join(WORKSPACE_LEASE_FILE)
    }

    fn avg_step_time_ms(&self) -> f32 {
        self.avg_step_time_micros.load(Ordering::SeqCst) as f32 / 1000.0
    }

    fn record_step_time(&self, elapsed: Duration) {
        let sample_micros = elapsed.as_micros().min(u64::MAX as u128) as u64;
        let current = self.avg_step_time_micros.load(Ordering::SeqCst);
        let next = if current == 0 {
            sample_micros
        } else {
            current.saturating_mul(9) / 10 + sample_micros / 10
        };
        self.avg_step_time_micros.store(next, Ordering::SeqCst);
    }

    async fn detail(&self) -> WorkspaceDetailResponse {
        let manifest = self.manifest.read().await.clone();
        let status = self.status_cache.read().await.clone();
        WorkspaceDetailResponse {
            summary: manifest.summary(&status, self.running.load(Ordering::SeqCst)),
            status,
        }
    }
}

pub struct RuntimeManager {
    config: RuntimeConfig,
    workspaces: RwLock<HashMap<WorkspaceKey, Arc<WorkspaceHandle>>>,
    semaphore: Arc<Semaphore>,
    autoscaler: Arc<dyn RuntimeAutoscaler>,
    autoscaler_report: RwLock<AutoscalerReport>,
    load_existing_lock: AsyncMutex<()>,
    scheduler_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    last_reconcile_ms: AtomicU64,
    last_autoscaler_ms: AtomicU64,
    #[cfg(feature = "sysinfo")]
    system: tokio::sync::Mutex<sysinfo::System>,
    stop_tx: watch::Sender<bool>,
}

impl RuntimeManager {
    pub async fn new(config: RuntimeConfig) -> anyhow::Result<Arc<Self>> {
        std::fs::create_dir_all(config.root_dir.join("users")).with_context(|| {
            format!(
                "failed to create runtime root '{}'",
                config.root_dir.join("users").display()
            )
        })?;
        std::fs::create_dir_all(config.root_dir.join(COORDINATION_DIR)).with_context(|| {
            format!(
                "failed to create runtime coordination dir '{}'",
                config.root_dir.join(COORDINATION_DIR).display()
            )
        })?;

        let autoscaler: Arc<dyn RuntimeAutoscaler> =
            if let Some(continuum) = config.continuum.clone() {
                Arc::new(ContinuumAutoscaler::new(
                    continuum,
                    config.local_worker_limit.max(1),
                    &config.root_dir,
                    config.orchestrator_addr.clone(),
                ))
            } else {
                Arc::new(NoopAutoscaler {
                    local_worker_limit: config.local_worker_limit.max(1),
                })
            };

        let (stop_tx, stop_rx) = watch::channel(false);
        let manager = Arc::new(Self {
            autoscaler_report: RwLock::new(AutoscalerReport {
                provider: if config.continuum.is_some() {
                    "continuum".to_string()
                } else {
                    "local".to_string()
                },
                enabled: config.continuum.is_some(),
                local_worker_limit: config.local_worker_limit.max(1),
                requested_remote_nodes: 0,
                active_remote_nodes: 0,
                last_action: None,
                controller_role: None,
                pressure_signals: Vec::new(),
                local_cpu_usage_pct: None,
                local_memory_usage_pct: None,
                local_avg_step_time_ms: None,
                cluster_nodes: None,
                cluster_avg_cpu_usage_pct: None,
                cluster_avg_step_time_ms: None,
                cluster_load_skew: None,
            }),
            semaphore: Arc::new(Semaphore::new(config.local_worker_limit.max(1))),
            workspaces: RwLock::new(HashMap::new()),
            autoscaler,
            load_existing_lock: AsyncMutex::new(()),
            scheduler_task: Mutex::new(None),
            last_reconcile_ms: AtomicU64::new(0),
            last_autoscaler_ms: AtomicU64::new(0),
            #[cfg(feature = "sysinfo")]
            system: tokio::sync::Mutex::new(sysinfo::System::new()),
            config,
            stop_tx,
        });

        manager.load_existing_workspaces().await?;
        manager.spawn_scheduler(stop_rx);
        Ok(manager)
    }

    pub fn root_dir(&self) -> &Path {
        &self.config.root_dir
    }

    pub async fn shutdown(&self) {
        let _ = self.stop_tx.send(true);
        let scheduler_task = self
            .scheduler_task
            .lock()
            .ok()
            .and_then(|mut guard| guard.take());
        if let Some(task) = scheduler_task {
            let _ = task.await;
        }
    }

    pub async fn runtime_status(&self, user_id: &str) -> anyhow::Result<RuntimeStatusResponse> {
        self.runtime_status_for_users(user_id, [user_id]).await
    }

    pub async fn runtime_status_for_users<I, S>(
        &self,
        user_id: &str,
        user_ids: I,
    ) -> anyhow::Result<RuntimeStatusResponse>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let user = sanitize_segment(user_id, "anonymous");
        let mut visible_users = Vec::new();
        let mut seen_users = HashSet::new();
        for owner in user_ids {
            let owner_id = sanitize_segment(owner.as_ref(), "anonymous");
            if seen_users.insert(owner_id.clone()) {
                visible_users.push(owner_id);
            }
        }
        if visible_users.is_empty() {
            visible_users.push("anonymous".to_string());
        }
        let total_users = visible_users.len();
        let workspaces = self
            .list_workspaces_for_users(visible_users.iter().map(String::as_str))
            .await?;
        let running_workspaces = workspaces
            .iter()
            .filter(|workspace| workspace.running)
            .count();
        let total_workspaces = workspaces.len();

        Ok(RuntimeStatusResponse {
            user_id: user,
            tick_interval_ms: self.config.tick_interval_ms,
            local_worker_limit: self.config.local_worker_limit.max(1),
            total_users,
            total_workspaces,
            running_workspaces,
            autoscaler: self.autoscaler_report.read().await.clone(),
            workspaces,
        })
    }

    pub async fn list_workspaces(&self, user_id: &str) -> anyhow::Result<Vec<WorkspaceSummary>> {
        self.list_workspaces_for_users([user_id]).await
    }

    pub async fn list_workspaces_for_users<I, S>(
        &self,
        user_ids: I,
    ) -> anyhow::Result<Vec<WorkspaceSummary>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.load_existing_workspaces().await?;
        let mut owner_order = Vec::new();
        let mut owner_filter = HashSet::new();
        for user_id in user_ids {
            let owner_id = sanitize_segment(user_id.as_ref(), "anonymous");
            if owner_filter.insert(owner_id.clone()) {
                owner_order.push(owner_id);
            }
        }
        if owner_order.is_empty() {
            owner_order.push("anonymous".to_string());
            owner_filter.insert("anonymous".to_string());
        }
        let handles = {
            let guard = self.workspaces.read().await;
            guard
                .iter()
                .filter(|(key, _)| owner_filter.contains(&key.user_id))
                .map(|(_, handle)| handle.clone())
                .collect::<Vec<_>>()
        };
        let owner_rank = owner_order
            .iter()
            .enumerate()
            .map(|(idx, owner)| (owner.clone(), idx))
            .collect::<HashMap<_, _>>();

        let mut summaries = Vec::with_capacity(handles.len());
        for handle in handles {
            self.maybe_refresh_workspace_from_disk(&handle).await?;
            let manifest = handle.manifest.read().await.clone();
            let status = handle.status_cache.read().await.clone();
            summaries.push(manifest.summary(&status, handle.running.load(Ordering::SeqCst)));
        }
        summaries.sort_by(|lhs, rhs| {
            owner_rank
                .get(&lhs.owner_id)
                .copied()
                .unwrap_or(usize::MAX)
                .cmp(&owner_rank.get(&rhs.owner_id).copied().unwrap_or(usize::MAX))
                .then(lhs.workspace_id.cmp(&rhs.workspace_id))
        });
        Ok(summaries)
    }

    pub async fn workspace_detail(
        &self,
        user_id: &str,
        workspace_id: &str,
    ) -> anyhow::Result<WorkspaceDetailResponse> {
        let handle = self.workspace_handle(user_id, workspace_id).await?;
        self.maybe_refresh_workspace_from_disk(&handle).await?;
        Ok(handle.detail().await)
    }

    pub async fn create_workspace(
        &self,
        user_id: &str,
        req: WorkspaceCreateRequest,
    ) -> anyhow::Result<WorkspaceDetailResponse> {
        let sanitized_user = sanitize_segment(user_id, "anonymous");
        let requested_id = req
            .workspace_id
            .as_deref()
            .map(|id| sanitize_segment(id, "workspace"))
            .filter(|id| !id.is_empty())
            .unwrap_or_else(|| generate_workspace_id(req.name.as_deref()));
        let key = WorkspaceKey {
            user_id: sanitized_user.clone(),
            workspace_id: requested_id.clone(),
        };

        {
            let guard = self.workspaces.read().await;
            if guard.contains_key(&key) {
                return Err(anyhow!("workspace '{}' already exists", requested_id));
            }
        }

        let dir = self.workspace_dir(&sanitized_user, &requested_id);
        if dir.exists() {
            return Err(anyhow!(
                "workspace directory '{}' already exists",
                dir.display()
            ));
        }

        let mut spec = EngineSpec::default();
        if let Some(model) = req.neuron_model.as_deref() {
            spec.neuron_model = model.to_string();
        }
        if let Some(learning_rule) = req.learning_rule.as_deref() {
            spec.learning_rule = learning_rule.to_string();
        }

        let mut engine = RunnerEngine::new(spec)?;
        if let Some(config_json) = req.config_json.as_deref() {
            engine.import_config_json(config_json)?;
        }
        if let Some(snapshot_json) = req.snapshot_json.as_deref() {
            engine.import_snapshot_json(snapshot_json)?;
        }

        let status = engine.status();
        let activity = engine.activity();
        let snapshot_json = engine.export_snapshot_json()?;
        let created_at_ms = now_ms();
        let manifest = WorkspaceManifest {
            version: 1,
            user_id: sanitized_user.clone(),
            workspace_id: requested_id.clone(),
            name: req.name.unwrap_or_else(|| requested_id.clone()),
            created_at_ms,
            updated_at_ms: created_at_ms,
            last_saved_at_ms: Some(created_at_ms),
            desired_running: req.auto_start.unwrap_or(false),
            autosave_steps: self.config.autosave_steps.max(1),
            engine: engine.spec().clone(),
        };

        let handle = Arc::new(WorkspaceHandle {
            key: key.clone(),
            dir,
            manifest: RwLock::new(manifest),
            engine: Mutex::new(engine),
            running: AtomicBool::new(req.auto_start.unwrap_or(false)),
            resume_suppressed: AtomicBool::new(false),
            stepping: AtomicBool::new(false),
            avg_step_time_micros: AtomicU64::new(0),
            status_cache: RwLock::new(status),
            activity_cache: RwLock::new(activity),
        });

        self.persist_workspace_files(&handle, &snapshot_json, true)
            .await
            .context("failed to persist new workspace")?;

        self.workspaces.write().await.insert(key, handle.clone());
        Ok(handle.detail().await)
    }

    pub async fn delete_workspace(
        &self,
        user_id: &str,
        workspace_id: &str,
    ) -> anyhow::Result<serde_json::Value> {
        let key = WorkspaceKey {
            user_id: sanitize_segment(user_id, "anonymous"),
            workspace_id: sanitize_segment(workspace_id, "workspace"),
        };
        let handle = self
            .workspaces
            .read()
            .await
            .get(&key)
            .cloned()
            .ok_or_else(|| anyhow!("workspace '{}' was not found", workspace_id))?;
        {
            let mut manifest = handle.manifest.write().await;
            manifest.desired_running = false;
            manifest.updated_at_ms = now_ms();
        }
        handle.running.store(false, Ordering::SeqCst);
        self.persist_manifest_only(&handle).await?;
        let _lease = self
            .acquire_workspace_lease(&handle, Duration::from_secs(5))
            .await?;
        self.workspaces.write().await.remove(&key);
        std::fs::remove_dir_all(&handle.dir)
            .with_context(|| format!("failed to delete '{}'", handle.dir.display()))?;
        Ok(serde_json::json!({ "ok": true, "workspace_id": workspace_id }))
    }

    pub async fn workspace_snapshot(
        &self,
        user_id: &str,
        workspace_id: &str,
    ) -> anyhow::Result<WorkspaceSnapshotResponse> {
        let handle = self.workspace_handle(user_id, workspace_id).await?;
        self.maybe_refresh_workspace_from_disk(&handle).await?;
        let workspace_id = handle.key.workspace_id.clone();
        let saved_at_ms = {
            let manifest = handle.manifest.read().await;
            manifest.last_saved_at_ms.or(Some(manifest.updated_at_ms))
        };
        let handle_for_export = handle.clone();
        let snapshot_json = tokio::task::spawn_blocking(move || -> anyhow::Result<String> {
            let engine = handle_for_export
                .engine
                .lock()
                .map_err(|_| anyhow!("workspace engine lock poisoned"))?;
            engine.export_snapshot_json()
        })
        .await
        .context("workspace snapshot task failed")??;

        Ok(WorkspaceSnapshotResponse {
            workspace_id,
            saved_at_ms,
            snapshot_json,
        })
    }

    pub async fn workspace_saved_snapshot(
        &self,
        user_id: &str,
        workspace_id: &str,
        if_saved_after_ms: Option<u64>,
    ) -> anyhow::Result<Option<WorkspaceSnapshotResponse>> {
        let handle = self.workspace_handle(user_id, workspace_id).await?;
        self.maybe_refresh_workspace_from_disk(&handle).await?;
        let workspace_id = handle.key.workspace_id.clone();
        let saved_at_ms = {
            let manifest = handle.manifest.read().await;
            manifest.last_saved_at_ms.or(Some(manifest.updated_at_ms))
        };

        if let (Some(if_saved_after_ms), Some(saved_at_ms)) = (if_saved_after_ms, saved_at_ms) {
            if saved_at_ms <= if_saved_after_ms {
                return Ok(None);
            }
        }

        let snapshot_path = handle.latest_snapshot_path();
        let snapshot_json = tokio::task::spawn_blocking(move || read_if_exists(&snapshot_path))
            .await
            .context("workspace saved snapshot task failed")??;

        if let Some(snapshot_json) = snapshot_json {
            return Ok(Some(WorkspaceSnapshotResponse {
                workspace_id,
                saved_at_ms,
                snapshot_json,
            }));
        }

        self.workspace_snapshot(user_id, workspace_id.as_str())
            .await
            .map(Some)
    }

    pub async fn workspace_activity(
        &self,
        user_id: &str,
        workspace_id: &str,
    ) -> anyhow::Result<WorkspaceActivityResponse> {
        let handle = self.workspace_handle(user_id, workspace_id).await?;
        self.maybe_refresh_workspace_from_disk(&handle).await?;
        let activity = handle.activity_cache.read().await.clone();
        Ok(WorkspaceActivityResponse {
            workspace_id: handle.key.workspace_id.clone(),
            activity,
        })
    }

    pub async fn import_workspace_json(
        &self,
        user_id: &str,
        workspace_id: &str,
        req: WorkspaceImportRequest,
    ) -> anyhow::Result<WorkspaceDetailResponse> {
        let handle = self.workspace_handle(user_id, workspace_id).await?;
        let _lease = self
            .acquire_workspace_lease(&handle, Duration::from_secs(2))
            .await?;
        let handle_for_import = handle.clone();
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let mut engine = handle_for_import
                .engine
                .lock()
                .map_err(|_| anyhow!("workspace engine lock poisoned"))?;
            if let Some(model) = req.neuron_model.as_deref() {
                engine.set_neuron_model_name(model)?;
            }
            if let Some(learning_rule) = req.learning_rule.as_deref() {
                engine.set_learning_rule_name(learning_rule)?;
            }
            engine.import_payload_json(
                &req.payload_json,
                req.kind.unwrap_or(EnginePayloadKind::Auto),
            )?;

            let snapshot_json = engine.export_snapshot_json()?;
            {
                let status = engine.status();
                let activity = engine.activity();
                *handle_for_import.status_cache.blocking_write() = status;
                *handle_for_import.activity_cache.blocking_write() = activity;
            }
            {
                let mut manifest = handle_for_import.manifest.blocking_write();
                manifest.updated_at_ms = now_ms();
                manifest.engine = engine.spec().clone();
                if req.auto_start.unwrap_or(false) {
                    manifest.desired_running = true;
                } else {
                    manifest.desired_running = false;
                }
                manifest.last_saved_at_ms = Some(now_ms());
            }

            handle_for_import
                .running
                .store(req.auto_start.unwrap_or(false), Ordering::SeqCst);
            persist_handle_files(
                &handle_for_import,
                &snapshot_json,
                req.replace_baseline.unwrap_or(false),
            )
        })
        .await
        .context("workspace import task failed")??;

        Ok(handle.detail().await)
    }

    pub async fn control_workspace(
        &self,
        user_id: &str,
        workspace_id: &str,
        action: WorkspaceControlAction,
    ) -> anyhow::Result<WorkspaceDetailResponse> {
        let handle = self.workspace_handle(user_id, workspace_id).await?;
        match action {
            WorkspaceControlAction::Start => {
                {
                    let mut manifest = handle.manifest.write().await;
                    manifest.desired_running = true;
                    manifest.updated_at_ms = now_ms();
                }
                handle.resume_suppressed.store(false, Ordering::SeqCst);
                handle.running.store(true, Ordering::SeqCst);
                self.persist_manifest_only(&handle).await?;
            }
            WorkspaceControlAction::Stop => {
                {
                    let mut manifest = handle.manifest.write().await;
                    manifest.desired_running = false;
                    manifest.updated_at_ms = now_ms();
                }
                handle.resume_suppressed.store(false, Ordering::SeqCst);
                handle.running.store(false, Ordering::SeqCst);
                self.persist_manifest_only(&handle).await?;
            }
            WorkspaceControlAction::Repeat => {
                let _lease = self
                    .acquire_workspace_lease(&handle, Duration::from_secs(2))
                    .await?;
                let handle_for_reset = handle.clone();
                tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
                    let baseline_path = handle_for_reset.baseline_snapshot_path();
                    let mut engine = handle_for_reset
                        .engine
                        .lock()
                        .map_err(|_| anyhow!("workspace engine lock poisoned"))?;
                    if let Some(baseline_json) = read_if_exists(&baseline_path)? {
                        engine.import_snapshot_json(&baseline_json)?;
                    } else {
                        engine.reset_from_spec()?;
                    }
                    let snapshot_json = engine.export_snapshot_json()?;
                    *handle_for_reset.status_cache.blocking_write() = engine.status();
                    *handle_for_reset.activity_cache.blocking_write() = engine.activity();
                    {
                        let mut manifest = handle_for_reset.manifest.blocking_write();
                        manifest.desired_running = true;
                        manifest.updated_at_ms = now_ms();
                        manifest.last_saved_at_ms = Some(now_ms());
                        manifest.engine = engine.spec().clone();
                    }
                    handle_for_reset
                        .resume_suppressed
                        .store(false, Ordering::SeqCst);
                    handle_for_reset.running.store(true, Ordering::SeqCst);
                    persist_handle_files(&handle_for_reset, &snapshot_json, false)
                })
                .await
                .context("workspace repeat task failed")??;
            }
            WorkspaceControlAction::Reset => {
                let _lease = self
                    .acquire_workspace_lease(&handle, Duration::from_secs(2))
                    .await?;
                let handle_for_reset = handle.clone();
                tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
                    let baseline_path = handle_for_reset.baseline_snapshot_path();
                    let mut engine = handle_for_reset
                        .engine
                        .lock()
                        .map_err(|_| anyhow!("workspace engine lock poisoned"))?;
                    if let Some(baseline_json) = read_if_exists(&baseline_path)? {
                        engine.import_snapshot_json(&baseline_json)?;
                    } else {
                        engine.reset_from_spec()?;
                    }
                    let snapshot_json = engine.export_snapshot_json()?;
                    *handle_for_reset.status_cache.blocking_write() = engine.status();
                    *handle_for_reset.activity_cache.blocking_write() = engine.activity();
                    {
                        let mut manifest = handle_for_reset.manifest.blocking_write();
                        manifest.desired_running = false;
                        manifest.updated_at_ms = now_ms();
                        manifest.last_saved_at_ms = Some(now_ms());
                        manifest.engine = engine.spec().clone();
                    }
                    handle_for_reset
                        .resume_suppressed
                        .store(false, Ordering::SeqCst);
                    handle_for_reset.running.store(false, Ordering::SeqCst);
                    persist_handle_files(&handle_for_reset, &snapshot_json, false)
                })
                .await
                .context("workspace reset task failed")??;
            }
            WorkspaceControlAction::New => {
                let _lease = self
                    .acquire_workspace_lease(&handle, Duration::from_secs(2))
                    .await?;
                let handle_for_new = handle.clone();
                tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
                    let existing = handle_for_new.manifest.blocking_read().clone();
                    let mut spec = existing.engine.clone();
                    let mut fresh_cfg = crate::config::NetworkConfig::default();
                    fresh_cfg.aarnn_layer_depth = existing.engine.net.aarnn_layer_depth;
                    spec.net = fresh_cfg;
                    let engine = RunnerEngine::new(spec)?;
                    let snapshot_json = engine.export_snapshot_json()?;
                    let status = engine.status();
                    let activity = engine.activity();
                    {
                        let mut manifest = handle_for_new.manifest.blocking_write();
                        manifest.desired_running = false;
                        manifest.updated_at_ms = now_ms();
                        manifest.last_saved_at_ms = Some(now_ms());
                        manifest.engine = engine.spec().clone();
                    }
                    *handle_for_new
                        .engine
                        .lock()
                        .map_err(|_| anyhow!("workspace engine lock poisoned"))? = engine;
                    *handle_for_new.status_cache.blocking_write() = status;
                    *handle_for_new.activity_cache.blocking_write() = activity;
                    handle_for_new
                        .resume_suppressed
                        .store(false, Ordering::SeqCst);
                    handle_for_new.running.store(false, Ordering::SeqCst);
                    persist_handle_files(&handle_for_new, &snapshot_json, true)
                })
                .await
                .context("workspace new task failed")??;
            }
            WorkspaceControlAction::Save => {
                self.save_workspace_handle(handle.clone()).await?;
            }
            WorkspaceControlAction::Step => {
                self.step_workspace_once(handle.clone()).await?;
            }
        }
        Ok(handle.detail().await)
    }

    fn spawn_scheduler(self: &Arc<Self>, mut stop_rx: watch::Receiver<bool>) {
        let manager = self.clone();
        let task = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_millis(
                manager.config.tick_interval_ms.max(1),
            ));
            ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = stop_rx.changed() => {
                        if *stop_rx.borrow() {
                            break;
                        }
                    }
                    _ = ticker.tick() => {
                        if let Err(err) = manager.scheduler_tick().await {
                            nm_err!("[warn] runtime scheduler tick failed: {}", err);
                        }
                    }
                }
            }
        });
        if let Ok(mut guard) = self.scheduler_task.lock() {
            *guard = Some(task);
        }
    }

    async fn scheduler_tick(&self) -> anyhow::Result<()> {
        let now = now_ms();
        if now.saturating_sub(self.last_reconcile_ms.load(Ordering::SeqCst))
            >= self.config.reconcile_interval_ms.max(100)
        {
            self.load_existing_workspaces().await?;
            self.last_reconcile_ms.store(now, Ordering::SeqCst);
        }

        let handles = {
            let guard = self.workspaces.read().await;
            guard.values().cloned().collect::<Vec<_>>()
        };
        for handle in &handles {
            self.maybe_refresh_workspace_from_disk(handle).await?;
        }

        let total_users = handles
            .iter()
            .map(|handle| handle.key.user_id.clone())
            .collect::<HashSet<_>>()
            .len();
        let running_handles = handles
            .iter()
            .filter(|handle| handle.running.load(Ordering::SeqCst))
            .cloned()
            .collect::<Vec<_>>();
        let running_workspaces = running_handles.len();
        let step_samples = running_handles
            .iter()
            .map(|handle| handle.avg_step_time_ms())
            .filter(|value| *value > 0.0)
            .collect::<Vec<_>>();
        let local_avg_step_time_ms = if step_samples.is_empty() {
            0.0
        } else {
            step_samples.iter().copied().sum::<f32>() / step_samples.len() as f32
        };

        if now.saturating_sub(self.last_autoscaler_ms.load(Ordering::SeqCst))
            >= self.config.autoscaler_interval_ms.max(250)
        {
            #[cfg(feature = "sysinfo")]
            let (local_cpu_usage_pct, local_memory_usage_pct) = {
                let mut system = self.system.lock().await;
                system.refresh_cpu_usage();
                system.refresh_memory();
                let cpu = system.global_cpu_usage();
                let memory = if system.total_memory() > 0 {
                    100.0
                        * (1.0 - (system.available_memory() as f32 / system.total_memory() as f32))
                } else {
                    0.0
                };
                (cpu, memory)
            };
            #[cfg(not(feature = "sysinfo"))]
            let (local_cpu_usage_pct, local_memory_usage_pct) = (0.0, 0.0);

            let metrics = RuntimeMetrics {
                total_users,
                total_workspaces: handles.len(),
                running_workspaces,
                local_worker_limit: self.config.local_worker_limit.max(1),
                local_cpu_usage_pct,
                local_memory_usage_pct,
                local_avg_step_time_ms,
                demand_ratio: running_workspaces as f32
                    / self.config.local_worker_limit.max(1) as f32,
            };

            let report = self
                .autoscaler
                .evaluate(metrics)
                .await
                .unwrap_or_else(|err| AutoscalerReport {
                    provider: "runtime-error".to_string(),
                    enabled: false,
                    local_worker_limit: self.config.local_worker_limit.max(1),
                    requested_remote_nodes: 0,
                    active_remote_nodes: 0,
                    last_action: Some(format!("autoscaler failed: {}", err)),
                    controller_role: None,
                    pressure_signals: Vec::new(),
                    local_cpu_usage_pct: Some(local_cpu_usage_pct),
                    local_memory_usage_pct: Some(local_memory_usage_pct),
                    local_avg_step_time_ms: Some(local_avg_step_time_ms),
                    cluster_nodes: None,
                    cluster_avg_cpu_usage_pct: None,
                    cluster_avg_step_time_ms: None,
                    cluster_load_skew: None,
                });
            *self.autoscaler_report.write().await = report;
            self.last_autoscaler_ms.store(now, Ordering::SeqCst);
        }

        for handle in handles {
            if !handle.running.load(Ordering::SeqCst) {
                continue;
            }
            if handle
                .stepping
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                continue;
            }
            let lease = match self.try_acquire_workspace_lease(&handle).await? {
                Some(lease) => lease,
                None => {
                    handle.stepping.store(false, Ordering::SeqCst);
                    continue;
                }
            };

            let permit = match self.semaphore.clone().acquire_owned().await {
                Ok(permit) => permit,
                Err(_) => {
                    handle.stepping.store(false, Ordering::SeqCst);
                    break;
                }
            };
            let handle_for_step = handle.clone();
            let autosave_steps = self.config.autosave_steps.max(1);
            tokio::spawn(async move {
                let _permit = permit;
                let _lease = lease;
                let handle_for_worker = handle_for_step.clone();
                let result = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
                    let step_started = std::time::Instant::now();
                    let mut engine = handle_for_worker
                        .engine
                        .lock()
                        .map_err(|_| anyhow!("workspace engine lock poisoned"))?;
                    let activity = engine.step(None);
                    let status = engine.status();
                    handle_for_worker.record_step_time(step_started.elapsed());
                    let should_persist =
                        autosave_steps == 1 || status.step % autosave_steps.max(1) == 0;
                    *handle_for_worker.status_cache.blocking_write() = status.clone();
                    *handle_for_worker.activity_cache.blocking_write() = activity;

                    {
                        let mut manifest = handle_for_worker.manifest.blocking_write();
                        manifest.updated_at_ms = now_ms();
                        manifest.engine = engine.spec().clone();
                    }

                    if should_persist {
                        let snapshot_json = engine.export_snapshot_json()?;
                        {
                            let mut manifest = handle_for_worker.manifest.blocking_write();
                            manifest.last_saved_at_ms = Some(now_ms());
                        }
                        persist_handle_files(&handle_for_worker, &snapshot_json, false)?;
                    } else {
                        persist_manifest(&handle_for_worker)?;
                    }
                    Ok(())
                })
                .await;

                if let Err(join_err) = result {
                    nm_err!("[warn] runtime workspace step join failed: {}", join_err);
                } else if let Ok(Err(step_err)) = result {
                    nm_err!("[warn] runtime workspace step failed: {}", step_err);
                }
                handle_for_step.stepping.store(false, Ordering::SeqCst);
            });
        }

        Ok(())
    }

    async fn step_workspace_once(&self, handle: Arc<WorkspaceHandle>) -> anyhow::Result<()> {
        let _lease = self
            .acquire_workspace_lease(&handle, Duration::from_secs(2))
            .await?;
        let handle_for_step = handle.clone();
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let step_started = std::time::Instant::now();
            let mut engine = handle_for_step
                .engine
                .lock()
                .map_err(|_| anyhow!("workspace engine lock poisoned"))?;
            let activity = engine.step(None);
            let status = engine.status();
            handle_for_step.record_step_time(step_started.elapsed());
            *handle_for_step.status_cache.blocking_write() = status;
            *handle_for_step.activity_cache.blocking_write() = activity;
            {
                let mut manifest = handle_for_step.manifest.blocking_write();
                manifest.updated_at_ms = now_ms();
                manifest.engine = engine.spec().clone();
            }
            let snapshot_json = engine.export_snapshot_json()?;
            {
                let mut manifest = handle_for_step.manifest.blocking_write();
                manifest.last_saved_at_ms = Some(now_ms());
            }
            persist_handle_files(&handle_for_step, &snapshot_json, false)
        })
        .await
        .context("workspace step task failed")??;
        Ok(())
    }

    async fn save_workspace_handle(&self, handle: Arc<WorkspaceHandle>) -> anyhow::Result<()> {
        let _lease = self
            .acquire_workspace_lease(&handle, Duration::from_secs(2))
            .await?;
        let handle_for_save = handle.clone();
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let engine = handle_for_save
                .engine
                .lock()
                .map_err(|_| anyhow!("workspace engine lock poisoned"))?;
            let snapshot_json = engine.export_snapshot_json()?;
            {
                let mut manifest = handle_for_save.manifest.blocking_write();
                manifest.updated_at_ms = now_ms();
                manifest.last_saved_at_ms = Some(now_ms());
                manifest.engine = engine.spec().clone();
            }
            persist_handle_files(&handle_for_save, &snapshot_json, false)
        })
        .await
        .context("workspace save task failed")??;
        Ok(())
    }

    async fn try_acquire_workspace_lease(
        &self,
        handle: &Arc<WorkspaceHandle>,
    ) -> anyhow::Result<Option<FileLease>> {
        let lease_path = handle.lease_path();
        tokio::task::spawn_blocking(move || -> anyhow::Result<Option<FileLease>> {
            try_acquire_lease(&lease_path)
        })
        .await
        .context("workspace lease task failed")?
    }

    async fn acquire_workspace_lease(
        &self,
        handle: &Arc<WorkspaceHandle>,
        timeout: Duration,
    ) -> anyhow::Result<FileLease> {
        acquire_lease_with_timeout(
            handle.lease_path(),
            timeout,
            Duration::from_millis(self.config.tick_interval_ms.max(5)),
        )
        .await
    }

    async fn maybe_refresh_workspace_from_disk(
        &self,
        handle: &Arc<WorkspaceHandle>,
    ) -> anyhow::Result<()> {
        if handle.stepping.load(Ordering::SeqCst) {
            return Ok(());
        }

        let manifest_path = handle.manifest_path();
        let disk_manifest = tokio::task::spawn_blocking({
            let manifest_path = manifest_path.clone();
            move || -> anyhow::Result<Option<WorkspaceManifest>> {
                let Some(raw) = read_if_exists(&manifest_path)? else {
                    return Ok(None);
                };
                let manifest =
                    serde_json::from_str::<WorkspaceManifest>(&raw).with_context(|| {
                        format!(
                            "failed to parse workspace manifest '{}'",
                            manifest_path.display()
                        )
                    })?;
                Ok(Some(manifest))
            }
        })
        .await
        .context("workspace manifest refresh task failed")??;
        let Some(disk_manifest) = disk_manifest else {
            return Ok(());
        };

        let (current_manifest_snapshot_ms, manifest_changed) = {
            let manifest = handle.manifest.read().await;
            (
                manifest
                    .last_saved_at_ms
                    .unwrap_or(manifest.updated_at_ms)
                    .max(manifest.updated_at_ms),
                manifest.updated_at_ms != disk_manifest.updated_at_ms
                    || manifest.last_saved_at_ms != disk_manifest.last_saved_at_ms
                    || manifest.desired_running != disk_manifest.desired_running
                    || manifest.name != disk_manifest.name,
            )
        };
        let snapshot_path = handle.latest_snapshot_path();
        let snapshot_mtime = tokio::task::spawn_blocking({
            let snapshot_path = snapshot_path.clone();
            move || file_modified_ms(&snapshot_path)
        })
        .await
        .context("workspace refresh stat task failed")??;

        let Some(snapshot_mtime) = snapshot_mtime else {
            *handle.manifest.write().await = disk_manifest.clone();
            handle.running.store(
                disk_manifest.desired_running && !handle.resume_suppressed.load(Ordering::SeqCst),
                Ordering::SeqCst,
            );
            return Ok(());
        };
        if snapshot_mtime <= current_manifest_snapshot_ms && !manifest_changed {
            return Ok(());
        }
        if snapshot_mtime <= current_manifest_snapshot_ms {
            *handle.manifest.write().await = disk_manifest.clone();
            handle.running.store(
                disk_manifest.desired_running && !handle.resume_suppressed.load(Ordering::SeqCst),
                Ordering::SeqCst,
            );
            return Ok(());
        }

        let handle_for_refresh = handle.clone();
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let snapshot_path = handle_for_refresh.latest_snapshot_path();
            let Some(snapshot_json) = read_if_exists(&snapshot_path)? else {
                return Ok(());
            };

            let mut engine = handle_for_refresh
                .engine
                .lock()
                .map_err(|_| anyhow!("workspace engine lock poisoned"))?;
            engine.import_snapshot_json(&snapshot_json)?;
            let status = engine.status();
            let activity = engine.activity();
            *handle_for_refresh.status_cache.blocking_write() = status;
            *handle_for_refresh.activity_cache.blocking_write() = activity;
            {
                let mut manifest = handle_for_refresh.manifest.blocking_write();
                *manifest = disk_manifest.clone();
                manifest.updated_at_ms = manifest.updated_at_ms.max(snapshot_mtime);
                manifest.last_saved_at_ms = Some(snapshot_mtime);
                manifest.engine = engine.spec().clone();
            }
            handle_for_refresh.running.store(
                disk_manifest.desired_running
                    && !handle_for_refresh.resume_suppressed.load(Ordering::SeqCst),
                Ordering::SeqCst,
            );
            Ok(())
        })
        .await
        .context("workspace refresh task failed")??;

        Ok(())
    }

    async fn persist_workspace_files(
        &self,
        handle: &Arc<WorkspaceHandle>,
        snapshot_json: &str,
        write_baseline: bool,
    ) -> anyhow::Result<()> {
        let handle = handle.clone();
        let snapshot_json = snapshot_json.to_string();
        tokio::task::spawn_blocking(move || {
            persist_handle_files(&handle, &snapshot_json, write_baseline)
        })
        .await
        .context("workspace persist task failed")??;
        Ok(())
    }

    async fn persist_manifest_only(&self, handle: &Arc<WorkspaceHandle>) -> anyhow::Result<()> {
        let handle = handle.clone();
        tokio::task::spawn_blocking(move || persist_manifest(&handle))
            .await
            .context("workspace manifest persist task failed")??;
        Ok(())
    }

    async fn workspace_handle(
        &self,
        user_id: &str,
        workspace_id: &str,
    ) -> anyhow::Result<Arc<WorkspaceHandle>> {
        let key = WorkspaceKey {
            user_id: sanitize_segment(user_id, "anonymous"),
            workspace_id: sanitize_segment(workspace_id, "workspace"),
        };
        if let Some(handle) = self.workspaces.read().await.get(&key).cloned() {
            return Ok(handle);
        }
        self.load_existing_workspaces().await?;
        self.workspaces
            .read()
            .await
            .get(&key)
            .cloned()
            .ok_or_else(|| anyhow!("workspace '{}' was not found", workspace_id))
    }

    fn workspace_dir(&self, user_id: &str, workspace_id: &str) -> PathBuf {
        self.config
            .root_dir
            .join("users")
            .join(user_id)
            .join("workspaces")
            .join(workspace_id)
    }

    async fn load_existing_workspaces(&self) -> anyhow::Result<()> {
        let _load_guard = self.load_existing_lock.lock().await;
        let users_root = self.config.root_dir.join("users");
        if !users_root.exists() {
            return Ok(());
        }

        let mut loaded = HashMap::new();
        for user_dir in read_dir_paths(&users_root, "runtime user")? {
            let workspaces_root = user_dir.join("workspaces");
            if !workspaces_root.exists() {
                continue;
            }
            for workspace_dir in match read_dir_paths(&workspaces_root, "runtime workspace") {
                Ok(paths) => paths,
                Err(err) => {
                    nm_err!(
                        "[warn] failed reading runtime workspaces dir '{}': {}",
                        workspaces_root.display(),
                        err
                    );
                    continue;
                }
            } {
                let manifest_path = workspace_dir.join(MANIFEST_FILE);
                if !manifest_path.exists() {
                    continue;
                }

                let manifest_raw = match std::fs::read_to_string(&manifest_path) {
                    Ok(raw) => raw,
                    Err(err) => {
                        nm_err!(
                            "[warn] failed reading workspace manifest '{}': {}",
                            manifest_path.display(),
                            err
                        );
                        continue;
                    }
                };
                let manifest: WorkspaceManifest = match serde_json::from_str(&manifest_raw) {
                    Ok(manifest) => manifest,
                    Err(err) => {
                        nm_err!(
                            "[warn] failed parsing workspace manifest '{}': {}",
                            manifest_path.display(),
                            err
                        );
                        continue;
                    }
                };

                let key = WorkspaceKey {
                    user_id: manifest.user_id.clone(),
                    workspace_id: manifest.workspace_id.clone(),
                };
                let mut engine = match RunnerEngine::new(manifest.engine.clone()) {
                    Ok(engine) => engine,
                    Err(err) => {
                        nm_err!(
                            "[warn] failed constructing runtime engine for '{}': {}",
                            manifest.workspace_id,
                            err
                        );
                        continue;
                    }
                };

                let latest_snapshot_path = workspace_dir.join(LATEST_SNAPSHOT_FILE);
                if let Ok(Some(snapshot_json)) = read_if_exists(&latest_snapshot_path) {
                    if let Err(err) = engine.import_snapshot_json(&snapshot_json) {
                        nm_err!(
                            "[warn] failed importing latest runtime snapshot '{}': {}",
                            latest_snapshot_path.display(),
                            err
                        );
                    }
                }

                let status = engine.status();
                let activity = engine.activity();
                let resume_suppressed =
                    !self.config.resume_existing_workspaces && manifest.desired_running;
                loaded.insert(
                    key.clone(),
                    Arc::new(WorkspaceHandle {
                        key,
                        dir: workspace_dir.clone(),
                        manifest: RwLock::new(manifest.clone()),
                        engine: Mutex::new(engine),
                        running: AtomicBool::new(manifest.desired_running && !resume_suppressed),
                        resume_suppressed: AtomicBool::new(resume_suppressed),
                        stepping: AtomicBool::new(false),
                        avg_step_time_micros: AtomicU64::new(0),
                        status_cache: RwLock::new(status),
                        activity_cache: RwLock::new(activity),
                    }),
                );
            }
        }

        let mut guard = self.workspaces.write().await;
        guard.retain(|key, _| loaded.contains_key(key));
        for (key, handle) in loaded {
            guard.entry(key).or_insert(handle);
        }
        Ok(())
    }
}

impl Drop for RuntimeManager {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(true);
    }
}

fn persist_manifest(handle: &WorkspaceHandle) -> anyhow::Result<()> {
    let manifest = handle.manifest.blocking_read().clone();
    let bytes =
        serde_json::to_vec_pretty(&manifest).context("failed to encode workspace manifest")?;
    atomic_write(&handle.manifest_path(), &bytes)
}

fn persist_handle_files(
    handle: &WorkspaceHandle,
    snapshot_json: &str,
    write_baseline: bool,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(&handle.dir)
        .with_context(|| format!("failed to create '{}'", handle.dir.display()))?;
    if write_baseline {
        atomic_write(
            handle.baseline_snapshot_path().as_path(),
            snapshot_json.as_bytes(),
        )?;
    }
    atomic_write(
        handle.latest_snapshot_path().as_path(),
        snapshot_json.as_bytes(),
    )?;
    persist_manifest(handle)?;
    Ok(())
}
