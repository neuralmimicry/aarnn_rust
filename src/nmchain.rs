use anyhow::{anyhow, Context};
use reqwest::{Client, Method};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct NmChainIdentityState {
    pub role: Option<String>,
    pub email: Option<String>,
    pub provider: Option<String>,
    pub subject: Option<String>,
    pub last_login_at: Option<String>,
    pub last_login_system: Option<String>,
    pub login_count: u64,
    pub updated_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct NmChainAccountSnapshot {
    pub scope: String,
    pub account_id: String,
    pub balance: i64,
    pub tokens: i64,
    pub paid_balance: i64,
    pub free_balance: i64,
    pub reserved: i64,
    pub available: i64,
    pub in_use: i64,
    pub last_topup_tokens: i64,
    pub capacity: i64,
    pub display_capacity: i64,
    pub low_threshold: i64,
    pub status: String,
    pub last_topup_at: Option<String>,
    pub updated_at: Option<String>,
    pub spent_total: i64,
    pub cashout_total: i64,
    pub shortfall_total: i64,
    pub free_grant_total: i64,
    pub identity: Option<NmChainIdentityState>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct NmChainLedgerEntry {
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
    pub meta: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct NmChainSubmitResult {
    pub duplicate: bool,
    pub tx_id: Option<String>,
    pub block_index: u64,
    pub chain_height: u64,
    pub head_hash: String,
    pub snapshot: Option<NmChainAccountSnapshot>,
    pub entry: Option<NmChainLedgerEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct NmChainLedgerResponse {
    pub entries: Vec<NmChainLedgerEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct NmChainIdentityUpsertRequest {
    pub request_id: Option<String>,
    pub user_id: String,
    pub role: Option<String>,
    pub email: Option<String>,
    pub provider: Option<String>,
    pub subject: Option<String>,
    #[serde(default)]
    pub meta: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct NmChainLoginObservedRequest {
    pub request_id: Option<String>,
    pub user_id: String,
    pub system: String,
    pub auth_mode: Option<String>,
    pub session_id: Option<String>,
    pub remote_addr: Option<String>,
    #[serde(default)]
    pub meta: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct NmChainTokenMutationRequest {
    pub request_id: Option<String>,
    pub account_scope: String,
    pub account_id: String,
    pub entry_type: String,
    pub delta: i64,
    #[serde(default)]
    pub meta: Value,
}

#[derive(Clone, Debug)]
pub struct NmChainClient {
    base_url: String,
    app_id: String,
    api_token: Option<String>,
    client: Client,
}

impl NmChainClient {
    pub fn new(
        base_url: impl Into<String>,
        app_id: impl Into<String>,
        api_token: Option<String>,
        timeout: Duration,
    ) -> anyhow::Result<Self> {
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .context("failed to build nmchain client")?;
        Ok(Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            app_id: app_id.into().trim().to_string(),
            api_token: api_token.map(|value| value.trim().to_string()).filter(|value| !value.is_empty()),
            client,
        })
    }

    pub fn configured(&self) -> bool {
        !self.base_url.is_empty()
    }

    pub async fn account_snapshot(
        &self,
        scope: &str,
        account_id: &str,
    ) -> anyhow::Result<NmChainAccountSnapshot> {
        self.request_json(Method::GET, &format!("/api/accounts/{}/{}", scope.trim(), account_id.trim()), None::<&Value>)
            .await
    }

    pub async fn ledger_entries(
        &self,
        scope: &str,
        account_id: &str,
        limit: usize,
    ) -> anyhow::Result<NmChainLedgerResponse> {
        let url = format!(
            "{}{}?limit={}",
            self.base_url,
            format!("/api/accounts/{}/{}/ledger", scope.trim(), account_id.trim()),
            limit.max(1)
        );
        let mut request = self.client.request(Method::GET, url);
        if let Some(token) = self.api_token.as_deref() {
            request = request.bearer_auth(token);
        }
        let response = request
            .send()
            .await
            .context("nmchain ledger request failed")?;
        decode_response(response).await
    }

    pub async fn upsert_identity(
        &self,
        payload: &NmChainIdentityUpsertRequest,
    ) -> anyhow::Result<NmChainSubmitResult> {
        self.request_json(Method::POST, "/api/events/identity", Some(payload))
            .await
    }

    pub async fn observe_login(
        &self,
        payload: &NmChainLoginObservedRequest,
    ) -> anyhow::Result<NmChainSubmitResult> {
        self.request_json(Method::POST, "/api/events/login", Some(payload))
            .await
    }

    pub async fn apply_token(
        &self,
        payload: &NmChainTokenMutationRequest,
    ) -> anyhow::Result<NmChainSubmitResult> {
        self.request_json(Method::POST, "/api/events/token", Some(payload))
            .await
    }

    async fn request_json<B: Serialize, T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> anyhow::Result<T> {
        let mut request = self.client.request(method, format!("{}{}", self.base_url, path));
        if let Some(token) = self.api_token.as_deref() {
            request = request.bearer_auth(token);
        }
        request = request.header("Accept", "application/json");
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request
            .send()
            .await
            .with_context(|| format!("nmchain request to '{}' failed", path))?;
        decode_response(response).await
    }

    pub fn app_id(&self) -> &str {
        &self.app_id
    }
}

async fn decode_response<T: DeserializeOwned>(response: reqwest::Response) -> anyhow::Result<T> {
    let status = response.status();
    let text = response
        .text()
        .await
        .unwrap_or_else(|_| "failed to read nmchain response".to_string());
    if status.is_success() {
        return serde_json::from_str(&text).context("failed to decode nmchain JSON response");
    }
    let message = serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .or_else(|| value.get("message").and_then(Value::as_str).map(ToOwned::to_owned))
        })
        .unwrap_or(text);
    Err(anyhow!("nmchain request failed ({}): {}", status, message))
}
