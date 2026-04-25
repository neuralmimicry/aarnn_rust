use crate::engine::{EngineActivity, EnginePayloadKind, EngineStatus};
use anyhow::{Context, anyhow};
use reqwest::Method;
use reqwest::blocking::{Client, RequestBuilder};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct AutoscalerReport {
    pub provider: String,
    pub enabled: bool,
    pub local_worker_limit: usize,
    pub requested_remote_nodes: usize,
    pub active_remote_nodes: usize,
    pub last_action: Option<String>,
    #[serde(default)]
    pub controller_role: Option<String>,
    #[serde(default)]
    pub pressure_signals: Vec<String>,
    #[serde(default)]
    pub local_cpu_usage_pct: Option<f32>,
    #[serde(default)]
    pub local_memory_usage_pct: Option<f32>,
    #[serde(default)]
    pub local_avg_step_time_ms: Option<f32>,
    #[serde(default)]
    pub cluster_nodes: Option<usize>,
    #[serde(default)]
    pub cluster_avg_cpu_usage_pct: Option<f32>,
    #[serde(default)]
    pub cluster_avg_step_time_ms: Option<f32>,
    #[serde(default)]
    pub cluster_load_skew: Option<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct WorkspaceSummary {
    pub workspace_id: String,
    pub network_id: String,
    pub name: String,
    pub running: bool,
    pub step: u64,
    pub sim_time_ms: f64,
    pub num_sensory_neurons: usize,
    pub num_hidden_layers: usize,
    pub num_output_neurons: usize,
    pub total_neurons: usize,
    pub desired_aarnn_depth: usize,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub last_saved_at_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct RuntimeStatusResponse {
    pub user_id: String,
    pub tick_interval_ms: u64,
    pub local_worker_limit: usize,
    pub total_users: usize,
    pub total_workspaces: usize,
    pub running_workspaces: usize,
    pub autoscaler: AutoscalerReport,
    pub workspaces: Vec<WorkspaceSummary>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct TokenBalanceResponse {
    pub configured: bool,
    pub balance: i64,
    pub tokens: i64,
    pub paid_balance: i64,
    pub free_balance: i64,
    pub available: i64,
    pub reserved: i64,
    pub in_use: i64,
    pub capacity: i64,
    pub display_capacity: i64,
    pub low_threshold: i64,
    pub status: String,
    pub last_topup_tokens: i64,
    pub last_topup_at: Option<String>,
    pub updated_at: Option<String>,
    pub spent_total: i64,
    pub cashout_total: i64,
    pub shortfall_total: i64,
    pub free_grant_total: i64,
    #[serde(default = "default_neuron_daily_rate")]
    pub neuron_daily_rate: i64,
    #[serde(default)]
    pub token_vault_url: Option<String>,
    #[serde(default)]
    pub buy_tokens_url: Option<String>,
    #[serde(default)]
    pub billing_dashboard_url: Option<String>,
    #[serde(default)]
    pub billing_admin_url: Option<String>,
    #[serde(default)]
    pub pricing: serde_json::Value,
    #[serde(default)]
    pub identity: Option<serde_json::Value>,
}

fn default_neuron_daily_rate() -> i64 {
    1
}

fn url_encode_form_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            b' ' => encoded.push('+'),
            _ => encoded.push_str(&format!("%{:02X}", byte)),
        }
    }
    encoded
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct TokenLedgerEntry {
    pub tx_id: String,
    pub request_id: Option<String>,
    pub block_index: u64,
    pub block_hash: String,
    pub ts: String,
    pub account_scope: String,
    pub account_id: String,
    #[serde(rename = "type")]
    pub entry_type: String,
    pub delta: i64,
    pub balance_after: i64,
    pub paid_after: i64,
    pub free_after: i64,
    pub reserved_after: i64,
    pub shortfall: i64,
    pub actor_app: String,
    #[serde(default)]
    pub meta: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct TokenLedgerResponse {
    pub configured: bool,
    pub entries: Vec<TokenLedgerEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct WorkspaceDetailResponse {
    pub summary: WorkspaceSummary,
    pub status: EngineStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct WorkspaceSnapshotResponse {
    pub workspace_id: String,
    #[serde(default)]
    pub saved_at_ms: Option<u64>,
    pub snapshot_json: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct WorkspaceActivityResponse {
    pub workspace_id: String,
    pub activity: EngineActivity,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct WorkspaceCreateRequest {
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub config_json: Option<String>,
    #[serde(default)]
    pub snapshot_json: Option<String>,
    #[serde(default)]
    pub neuron_model: Option<String>,
    #[serde(default)]
    pub learning_rule: Option<String>,
    #[serde(default)]
    pub auto_start: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceImportRequest {
    pub payload_json: String,
    #[serde(default)]
    pub kind: Option<EnginePayloadKind>,
    #[serde(default)]
    pub replace_baseline: Option<bool>,
    #[serde(default)]
    pub auto_start: Option<bool>,
    #[serde(default)]
    pub neuron_model: Option<String>,
    #[serde(default)]
    pub learning_rule: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceControlAction {
    Start,
    Stop,
    Repeat,
    Reset,
    New,
    Save,
    Step,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceControlRequest {
    pub action: WorkspaceControlAction,
}

#[derive(Clone, Debug, Deserialize)]
struct AuthModeResponse {
    mode: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuntimeAuthMode {
    Unknown,
    None,
    Local,
    Oidc,
}

pub struct BlockingRuntimeClient {
    base_url: String,
    client: Client,
    runtime_user: Option<String>,
    runtime_password: Option<String>,
    runtime_access_token: Option<String>,
    auth_mode: RuntimeAuthMode,
    authenticated: bool,
}

#[derive(Clone, Debug)]
pub struct RemoteWorkspaceBinding {
    pub base_url: String,
    pub user_id: Option<String>,
    pub password: Option<String>,
    pub access_token: Option<String>,
    pub workspace_id: String,
    pub save_on_exit: bool,
}

impl RemoteWorkspaceBinding {
    pub fn client(&self) -> anyhow::Result<BlockingRuntimeClient> {
        BlockingRuntimeClient::new(
            self.base_url.clone(),
            self.user_id.clone(),
            self.password.clone(),
            self.access_token.clone(),
        )
    }
}

impl BlockingRuntimeClient {
    pub fn new(
        base_url: impl Into<String>,
        runtime_user: Option<String>,
        runtime_password: Option<String>,
        runtime_access_token: Option<String>,
    ) -> anyhow::Result<Self> {
        let client = Client::builder()
            .cookie_store(true)
            .timeout(Duration::from_secs(30))
            .build()
            .context("failed to build runtime HTTP client")?;
        Ok(Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            client,
            runtime_user,
            runtime_password,
            runtime_access_token,
            auth_mode: RuntimeAuthMode::Unknown,
            authenticated: false,
        })
    }

    pub fn runtime_status(&mut self) -> anyhow::Result<RuntimeStatusResponse> {
        self.request_json(Method::GET, "/api/runtime/status")
    }

    pub fn token_balance(&mut self) -> anyhow::Result<TokenBalanceResponse> {
        self.request_json(Method::GET, "/api/tokens")
    }

    pub fn token_ledger(&mut self, limit: usize) -> anyhow::Result<TokenLedgerResponse> {
        self.request_json(
            Method::GET,
            &format!("/api/tokens/ledger?limit={}", limit.max(1)),
        )
    }

    pub fn list_workspaces(&mut self) -> anyhow::Result<Vec<WorkspaceSummary>> {
        self.request_json(Method::GET, "/api/runtime/workspaces")
    }

    pub fn create_workspace(
        &mut self,
        req: &WorkspaceCreateRequest,
    ) -> anyhow::Result<WorkspaceDetailResponse> {
        self.request_json_with_body(Method::POST, "/api/runtime/workspaces", req)
    }

    pub fn workspace_detail(
        &mut self,
        workspace_id: &str,
    ) -> anyhow::Result<WorkspaceDetailResponse> {
        self.request_json(
            Method::GET,
            &format!("/api/runtime/workspaces/{}", workspace_id),
        )
    }

    pub fn delete_workspace(&mut self, workspace_id: &str) -> anyhow::Result<serde_json::Value> {
        self.request_json(
            Method::DELETE,
            &format!("/api/runtime/workspaces/{}", workspace_id),
        )
    }

    pub fn workspace_snapshot(
        &mut self,
        workspace_id: &str,
    ) -> anyhow::Result<WorkspaceSnapshotResponse> {
        self.request_json(
            Method::GET,
            &format!("/api/runtime/workspaces/{}/snapshot", workspace_id),
        )
    }

    pub fn workspace_activity(
        &mut self,
        workspace_id: &str,
    ) -> anyhow::Result<WorkspaceActivityResponse> {
        self.request_json(
            Method::GET,
            &format!("/api/runtime/workspaces/{}/activity", workspace_id),
        )
    }

    pub fn control_workspace(
        &mut self,
        workspace_id: &str,
        action: WorkspaceControlAction,
    ) -> anyhow::Result<WorkspaceDetailResponse> {
        self.request_json_with_body(
            Method::POST,
            &format!("/api/runtime/workspaces/{}/control", workspace_id),
            &WorkspaceControlRequest { action },
        )
    }

    pub fn import_workspace(
        &mut self,
        workspace_id: &str,
        req: &WorkspaceImportRequest,
    ) -> anyhow::Result<WorkspaceDetailResponse> {
        self.request_json_with_body(
            Method::POST,
            &format!("/api/runtime/workspaces/{}/import", workspace_id),
            req,
        )
    }

    fn request_json<T: DeserializeOwned>(
        &mut self,
        method: Method,
        path: &str,
    ) -> anyhow::Result<T> {
        self.ensure_session()?;
        let response = self
            .build_request(method, path)
            .send()
            .with_context(|| format!("runtime request to '{}' failed", self.url_for(path)))?;
        self.decode_response(response)
    }

    fn request_json_with_body<B: Serialize, T: DeserializeOwned>(
        &mut self,
        method: Method,
        path: &str,
        body: &B,
    ) -> anyhow::Result<T> {
        self.ensure_session()?;
        let response = self
            .build_request(method, path)
            .json(body)
            .send()
            .with_context(|| format!("runtime request to '{}' failed", self.url_for(path)))?;
        self.decode_response(response)
    }

    fn ensure_session(&mut self) -> anyhow::Result<()> {
        if self.auth_mode == RuntimeAuthMode::Unknown {
            let auth: AuthModeResponse = self
                .client
                .get(self.url_for("/api/auth/mode"))
                .send()
                .context("failed to query runtime auth mode")?
                .json()
                .context("failed to decode runtime auth mode")?;
            self.auth_mode = match auth.mode.as_str() {
                "none" => RuntimeAuthMode::None,
                "local" => RuntimeAuthMode::Local,
                "oidc" => RuntimeAuthMode::Oidc,
                _ => RuntimeAuthMode::Unknown,
            };
        }

        if self.authenticated {
            return Ok(());
        }

        if self.auth_mode != RuntimeAuthMode::None {
            if let Some(access_token) = self
                .runtime_access_token
                .clone()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
            {
                if self.try_access_token_exchange(&access_token)? {
                    self.authenticated = true;
                    return Ok(());
                }
            }
        }

        match self.auth_mode {
            RuntimeAuthMode::None => {
                self.authenticated = true;
                Ok(())
            }
            RuntimeAuthMode::Local => {
                let username = self
                    .runtime_user
                    .clone()
                    .ok_or_else(|| anyhow!("runtime username is required for local auth"))?;
                let password = self
                    .runtime_password
                    .clone()
                    .ok_or_else(|| anyhow!("runtime password is required for local auth"))?;
                let response = self
                    .client
                    .post(self.url_for("/api/login"))
                    .json(&serde_json::json!({ "username": username, "password": password }))
                    .send()
                    .context("failed to authenticate to runtime API")?;
                if !response.status().is_success() {
                    let text = response
                        .text()
                        .unwrap_or_else(|_| "login failed".to_string());
                    return Err(anyhow!("runtime login failed: {}", text));
                }
                self.authenticated = true;
                Ok(())
            }
            RuntimeAuthMode::Oidc => Err(anyhow!(
                "runtime API is using OIDC; provide a shared access token with --runtime-access-token or NM_RUNTIME_ACCESS_TOKEN"
            )),
            RuntimeAuthMode::Unknown => Err(anyhow!("runtime auth mode is unknown")),
        }
    }

    fn try_access_token_exchange(&mut self, access_token: &str) -> anyhow::Result<bool> {
        let body = format!(
            "access_token={}&next=%2Fapi%2Fme",
            url_encode_form_component(access_token)
        );
        let response = self
            .client
            .post(self.url_for("/auth/access/exchange"))
            .header("content-type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .context("failed to exchange runtime access token")?;
        if !response.status().is_success() {
            return Ok(false);
        }
        let payload = match response.json::<serde_json::Value>() {
            Ok(payload) => payload,
            Err(_) => return Ok(false),
        };
        let authenticated = payload
            .get("authenticated")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let username_present = payload
            .get("username")
            .or_else(|| payload.get("user"))
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .map(|value| !value.is_empty())
            .unwrap_or(false);
        Ok(authenticated || username_present)
    }

    fn build_request(&self, method: Method, path: &str) -> RequestBuilder {
        let mut builder = self.client.request(method, self.url_for(path));
        if self.auth_mode == RuntimeAuthMode::None {
            if let Some(user_id) = self.runtime_user.as_deref() {
                if !user_id.trim().is_empty() {
                    builder = builder.header("x-nm-runtime-user", user_id.trim());
                }
            }
        }
        builder
    }

    fn url_for(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn decode_response<T: DeserializeOwned>(
        &self,
        response: reqwest::blocking::Response,
    ) -> anyhow::Result<T> {
        if response.status().is_success() {
            return response
                .json()
                .context("failed to decode runtime response body");
        }

        let status = response.status();
        let text = response
            .text()
            .unwrap_or_else(|_| "failed to read error body".to_string());
        let message = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|value| {
                value
                    .get("error")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .unwrap_or(text);
        Err(anyhow!("runtime request failed ({}): {}", status, message))
    }
}
