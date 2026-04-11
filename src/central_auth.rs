use anyhow::Context;
use reqwest::{Client, Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fmt;
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct CentralSessionResponse {
    pub authenticated: bool,
    pub user: Option<String>,
    pub role: Option<String>,
    #[serde(default)]
    pub groups: Vec<String>,
    pub email: Option<String>,
    #[serde(default)]
    pub active_team: Option<Value>,
    pub team_count: Option<i64>,
    pub pending_invitation_count: Option<i64>,
    pub is_admin: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct CentralLoginResponse {
    pub status: Option<String>,
    pub user: Option<String>,
    pub role: Option<String>,
    #[serde(default)]
    pub groups: Vec<String>,
    pub email: Option<String>,
    #[serde(default)]
    pub active_team: Option<Value>,
    pub team_count: Option<i64>,
    pub pending_invitation_count: Option<i64>,
    pub is_admin: Option<bool>,
    pub sso_token: Option<String>,
    pub sso_expires_in: Option<i64>,
    pub access_token: Option<String>,
    pub access_expires_at: Option<String>,
    pub error: Option<String>,
    pub details: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct CentralTokenSnapshot {
    pub balance: i64,
    pub tokens: i64,
    pub paid_balance: i64,
    pub free_balance: i64,
    pub available: i64,
    pub reserved: i64,
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
    pub scope: Option<String>,
    pub identity: Option<Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct CentralTokenLedgerEntry {
    pub ts: Option<String>,
    #[serde(rename = "type")]
    pub entry_type: String,
    pub user: Option<String>,
    pub delta: i64,
    pub balance_after: i64,
    #[serde(default)]
    pub meta: Value,
    #[serde(default)]
    pub shortfall: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct CentralTokenLedgerResponse {
    #[serde(default)]
    pub entries: Vec<CentralTokenLedgerEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct CentralTokenActionResponse {
    pub message: Option<String>,
    pub error: Option<String>,
    pub details: Option<String>,
    pub requested: Option<i64>,
    pub used: Option<i64>,
    pub shortfall: Option<i64>,
    pub entry: Option<CentralTokenLedgerEntry>,
    #[serde(flatten)]
    pub snapshot: CentralTokenSnapshot,
}

#[derive(Clone, Debug)]
pub struct CentralAuthClient {
    base_url: String,
    client: Client,
}

#[derive(Clone, Debug)]
pub struct CentralApiError {
    pub status: Option<u16>,
    pub message: String,
    pub payload: Value,
}

impl CentralApiError {
    fn from_response(status: StatusCode, payload: Value) -> Self {
        let message = payload
            .get("details")
            .and_then(Value::as_str)
            .or_else(|| payload.get("error").and_then(Value::as_str))
            .map(str::to_string)
            .unwrap_or_else(|| {
                format!("central auth request failed with HTTP {}", status.as_u16())
            });
        Self {
            status: Some(status.as_u16()),
            message,
            payload,
        }
    }

    fn from_client_error(message: impl Into<String>) -> Self {
        Self {
            status: None,
            message: message.into(),
            payload: json!({}),
        }
    }
}

impl fmt::Display for CentralApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CentralApiError {}

impl CentralAuthClient {
    pub fn new(base_url: impl Into<String>, timeout: Duration) -> anyhow::Result<Self> {
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .context("failed to build central auth client")?;
        Ok(Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            client,
        })
    }

    pub fn configured(&self) -> bool {
        !self.base_url.is_empty()
    }

    pub async fn session(
        &self,
        access_token: &str,
    ) -> Result<CentralSessionResponse, CentralApiError> {
        self.request_json(
            Method::GET,
            "/api/session",
            None::<&Value>,
            Some(access_token),
        )
        .await
    }

    pub async fn login(
        &self,
        username: &str,
        password: &str,
    ) -> Result<CentralLoginResponse, CentralApiError> {
        self.request_json(
            Method::POST,
            "/api/login",
            Some(&json!({
                "username": username.trim(),
                "password": password,
            })),
            None,
        )
        .await
    }

    pub async fn oidc_exchange(
        &self,
        id_token: &str,
        access_token: Option<&str>,
    ) -> Result<CentralLoginResponse, CentralApiError> {
        self.request_json(
            Method::POST,
            "/api/oidc/exchange",
            Some(&json!({
                "id_token": id_token,
                "access_token": access_token.unwrap_or(""),
            })),
            None,
        )
        .await
    }

    pub async fn token_snapshot(
        &self,
        access_token: &str,
    ) -> Result<CentralTokenSnapshot, CentralApiError> {
        self.request_json(
            Method::GET,
            "/api/tokens",
            None::<&Value>,
            Some(access_token),
        )
        .await
    }

    pub async fn token_ledger(
        &self,
        access_token: &str,
        limit: usize,
    ) -> Result<CentralTokenLedgerResponse, CentralApiError> {
        self.request_json(
            Method::GET,
            &format!("/api/tokens/ledger?limit={}", limit.max(1)),
            None::<&Value>,
            Some(access_token),
        )
        .await
    }

    pub async fn debit_tokens(
        &self,
        access_token: &str,
        amount: i64,
        request_id: &str,
        meta: Value,
    ) -> Result<CentralTokenActionResponse, CentralApiError> {
        self.request_json(
            Method::POST,
            "/api/tokens",
            Some(&json!({
                "action": "debit",
                "token_amount": amount,
                "request_id": request_id,
                "source": meta.get("source").and_then(Value::as_str).unwrap_or("aarnn"),
                "note": meta.get("note").and_then(Value::as_str),
                "operation": meta.get("operation").and_then(Value::as_str),
                "workspace_id": meta.get("workspace_id").and_then(Value::as_str),
            })),
            Some(access_token),
        )
        .await
    }

    pub async fn refund_tokens(
        &self,
        access_token: &str,
        amount: i64,
        request_id: &str,
        meta: Value,
    ) -> Result<CentralTokenActionResponse, CentralApiError> {
        self.request_json(
            Method::POST,
            "/api/tokens",
            Some(&json!({
                "action": "refund",
                "token_amount": amount,
                "request_id": request_id,
                "source": meta.get("source").and_then(Value::as_str).unwrap_or("aarnn"),
                "note": meta.get("note").and_then(Value::as_str),
                "operation": meta.get("operation").and_then(Value::as_str),
                "workspace_id": meta.get("workspace_id").and_then(Value::as_str),
            })),
            Some(access_token),
        )
        .await
    }

    async fn request_json<B: Serialize, T: for<'de> Deserialize<'de>>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
        bearer_token: Option<&str>,
    ) -> Result<T, CentralApiError> {
        if !self.configured() {
            return Err(CentralApiError::from_client_error(
                "central auth client is not configured",
            ));
        }
        let mut request = self
            .client
            .request(method, format!("{}{}", self.base_url, path))
            .header("Accept", "application/json");
        if let Some(token) = bearer_token
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            request = request.bearer_auth(token);
        }
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request
            .send()
            .await
            .map_err(|err| CentralApiError::from_client_error(err.to_string()))?;
        decode_response(response).await
    }
}

async fn decode_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
) -> Result<T, CentralApiError> {
    let status = response.status();
    let payload = response.json::<Value>().await.unwrap_or_else(|_| json!({}));
    if !status.is_success() {
        return Err(CentralApiError::from_response(status, payload));
    }
    serde_json::from_value(payload)
        .map_err(|err| CentralApiError::from_client_error(err.to_string()))
}
