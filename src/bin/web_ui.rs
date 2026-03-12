#![recursion_limit = "2048"]

use aarnn_rust::distributed::proto::{
    control_update, distributed_neuromorphic_client::DistributedNeuromorphicClient,
    network_update_request, ConfigUpdate, ControlUpdate, NetworkActivityRequest,
    NetworkSnapshotRequest, NetworkUpdateRequest, SpikeBatch, StatusRequest,
};
use aarnn_rust::distributed::EXTERNAL_SENSORY_LAYER_INDEX;
use argon2::password_hash::rand_core::OsRng;
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::{DefaultBodyLimit, Form, Query, State},
    http::{HeaderValue, StatusCode},
    middleware,
    response::{IntoResponse, Redirect},
    routing::{get, post},
    Extension, Json, Router,
};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use clap::Parser;
use futures_util::StreamExt;
use openidconnect::{
    core::{
        CoreAuthenticationFlow, CoreClient, CoreIdToken, CoreProviderMetadata, CoreUserInfoClaims,
    },
    reqwest, AccessToken, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet,
    EndpointNotSet, EndpointSet, IssuerUrl, Nonce, PkceCodeChallenge, PkceCodeVerifier,
    RedirectUrl, Scope, TokenResponse,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::fs;
use tokio::sync::{mpsc, RwLock};
use tokio_stream::wrappers::ReceiverStream;
use tonic::Request;

type OidcClient = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

const WEB_UI_GRPC_MAX_MESSAGE_BYTES: usize = 64 * 1024 * 1024;

async fn connect_cluster_client(
    addr: String,
) -> Result<DistributedNeuromorphicClient<tonic::transport::Channel>, tonic::transport::Error> {
    let client = DistributedNeuromorphicClient::connect(addr).await?;
    Ok(client
        .max_decoding_message_size(WEB_UI_GRPC_MAX_MESSAGE_BYTES)
        .max_encoding_message_size(WEB_UI_GRPC_MAX_MESSAGE_BYTES))
}

#[derive(Parser, Debug)]
struct Args {
    /// Listen address for the web UI server.
    #[arg(long, default_value = "0.0.0.0:8080")]
    listen: String,

    /// Default orchestrator address (e.g., http://host:50051).
    #[arg(long)]
    orchestrator: Option<String>,

    /// Auth mode: none, local, oidc.
    #[arg(long, default_value = "none")]
    auth_mode: String,

    /// User database file path.
    #[arg(long, default_value = "data/users.json")]
    users_file: String,

    /// Allow local signup via the web UI (auth_mode=local).
    #[arg(long, default_value_t = false)]
    allow_signup: bool,

    /// Session TTL in seconds.
    #[arg(long, default_value_t = 86400)]
    session_ttl_secs: u64,

    /// Bootstrap local user (auth_mode=local).
    #[arg(long)]
    local_user: Option<String>,

    /// Bootstrap local password (auth_mode=local).
    #[arg(long)]
    local_pass: Option<String>,

    /// OIDC issuer URL (auth_mode=oidc).
    #[arg(long)]
    oidc_issuer: Option<String>,

    /// OIDC client ID (auth_mode=oidc).
    #[arg(long)]
    oidc_client_id: Option<String>,

    /// OIDC client secret (auth_mode=oidc).
    #[arg(long)]
    oidc_client_secret: Option<String>,

    /// OIDC redirect URL (auth_mode=oidc).
    #[arg(long)]
    oidc_redirect_url: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AuthMode {
    None,
    Local,
    Oidc,
}

impl AuthMode {
    fn parse(raw: &str) -> Self {
        match raw.to_lowercase().as_str() {
            "local" => AuthMode::Local,
            "oidc" => AuthMode::Oidc,
            _ => AuthMode::None,
        }
    }
    fn as_str(&self) -> &'static str {
        match self {
            AuthMode::None => "none",
            AuthMode::Local => "local",
            AuthMode::Oidc => "oidc",
        }
    }
}

#[derive(Clone)]
struct AppState {
    default_orchestrator: Option<String>,
    auth: AuthConfig,
    users: Arc<RwLock<UserStore>>,
    sessions: Arc<RwLock<HashMap<String, SessionInfo>>>,
}

#[derive(Clone)]
struct AuthConfig {
    mode: AuthMode,
    users_file: String,
    allow_signup: bool,
    session_ttl_secs: u64,
    oidc: Option<OidcConfig>,
}

fn env_opt(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn apply_env_overrides(args: &mut Args) {
    if args.oidc_issuer.is_none() {
        args.oidc_issuer = env_opt("NM_OIDC_ISSUER");
    }
    if args.oidc_client_id.is_none() {
        args.oidc_client_id = env_opt("NM_OIDC_CLIENT_ID");
    }
    if args.oidc_client_secret.is_none() {
        args.oidc_client_secret = env_opt("NM_OIDC_CLIENT_SECRET");
    }
    if args.oidc_redirect_url.is_none() {
        args.oidc_redirect_url =
            env_opt("NM_OIDC_REDIRECT_URI").or_else(|| env_opt("NM_OIDC_REDIRECT_URL"));
    }

    if args.auth_mode.trim().eq_ignore_ascii_case("none") {
        if let Some(mode) = env_opt("NM_AUTH_MODE").or_else(|| env_opt("NM_OIDC_AUTH_MODE")) {
            args.auth_mode = mode;
        } else if args.oidc_issuer.is_some() {
            args.auth_mode = "oidc".to_string();
        }
    }
}

#[derive(Clone)]
struct OidcConfig {
    client: OidcClient,
    http_client: reqwest::Client,
    issuer: String,
    pending: Arc<RwLock<HashMap<String, OidcPending>>>,
}

struct OidcPending {
    nonce: Nonce,
    pkce_verifier: PkceCodeVerifier,
}

#[derive(Clone, Serialize, Deserialize)]
struct UserRecord {
    username: String,
    password_hash: Option<String>,
    oidc_subject: Option<String>,
    oidc_issuer: Option<String>,
    email: Option<String>,
    created_at: u64,
    config: Option<serde_json::Value>,
}

#[derive(Default, Serialize, Deserialize)]
struct UserStore {
    users: Vec<UserRecord>,
}

#[derive(Clone)]
struct SessionInfo {
    username: String,
    expires_at: u64,
}

#[derive(Clone)]
struct AuthUser {
    username: String,
}

impl UserStore {
    async fn load(path: &str) -> Self {
        match fs::read_to_string(path).await {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
            Err(_) => UserStore::default(),
        }
    }

    async fn save(&self, path: &str) -> anyhow::Result<()> {
        if let Some(parent) = std::path::Path::new(path).parent() {
            let _ = fs::create_dir_all(parent).await;
        }
        let raw = serde_json::to_string_pretty(self)?;
        fs::write(path, raw).await?;
        Ok(())
    }

    fn find_by_username_mut(&mut self, username: &str) -> Option<&mut UserRecord> {
        self.users.iter_mut().find(|u| u.username == username)
    }

    fn find_by_username(&self, username: &str) -> Option<&UserRecord> {
        self.users.iter().find(|u| u.username == username)
    }

    fn find_by_oidc_mut(&mut self, issuer: &str, subject: &str) -> Option<&mut UserRecord> {
        self.users.iter_mut().find(|u| {
            u.oidc_issuer.as_deref() == Some(issuer) && u.oidc_subject.as_deref() == Some(subject)
        })
    }

    fn ensure_unique_username(&self, base: &str) -> String {
        if self.find_by_username(base).is_none() {
            return base.to_string();
        }
        for idx in 1..1000 {
            let candidate = format!("{}{}", base, idx);
            if self.find_by_username(&candidate).is_none() {
                return candidate;
            }
        }
        format!("{}-{}", base, now_ts())
    }
}

fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs()
}

fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    Ok(argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("password hash failed: {e}"))?
        .to_string())
}

fn verify_password(hash: &str, password: &str) -> bool {
    let parsed = match PasswordHash::new(hash) {
        Ok(parsed) => parsed,
        Err(_) => return false,
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

fn new_session_id() -> String {
    let mut buf = [0u8; 32];
    fastrand::fill(&mut buf);
    hex::encode(buf)
}

fn session_cookie(session_id: &str, ttl_secs: u64) -> Cookie<'static> {
    let mut cookie = Cookie::new("nm_session", session_id.to_string());
    cookie.set_http_only(true);
    cookie.set_same_site(axum_extra::extract::cookie::SameSite::Lax);
    cookie.set_path("/");
    cookie.set_max_age(time::Duration::seconds(ttl_secs as i64));
    cookie
}

fn safe_next_path(value: Option<String>) -> String {
    let candidate = value.unwrap_or_default().trim().to_string();
    if candidate.starts_with('/') && !candidate.starts_with("//") && !candidate.contains("://") {
        return candidate;
    }
    "/".to_string()
}

#[derive(Deserialize)]
struct StatusQuery {
    addr: Option<String>,
}

#[derive(Deserialize)]
struct SnapshotQuery {
    addr: Option<String>,
    network_id: Option<String>,
    node_id: Option<String>,
}

#[derive(Deserialize)]
struct ActivityQuery {
    addr: Option<String>,
    network_id: Option<String>,
    node_id: Option<String>,
}

#[derive(Deserialize)]
struct UpdateNetworkPayload {
    addr: Option<String>,
    network_id: String,
    config_json: Option<String>,
    neuron_model: Option<String>,
    learning_rule: Option<String>,
}

#[derive(Deserialize)]
struct ControlNetworkPayload {
    addr: Option<String>,
    network_id: String,
    action: String,
}

#[derive(Deserialize)]
struct ExportQuery {
    addr: Option<String>,
    network_id: Option<String>,
    format: String,
}

#[derive(Deserialize)]
struct AerInjectPayload {
    addr: Option<String>,
    network_id: String,
    node_id: Option<String>,
    step_index: Option<i64>,
    aer_base: Option<u32>,
    aer_payload_hex: Option<String>,
    spike_indices: Option<Vec<u32>>,
    is_backward: Option<bool>,
}

#[derive(Deserialize)]
struct AerStreamQuery {
    addr: Option<String>,
    network_id: Option<String>,
    node_id: Option<String>,
    step_index: Option<i64>,
    aer_base: Option<u32>,
    is_backward: Option<bool>,
}

#[derive(Deserialize)]
struct AerStreamFrame {
    network_id: Option<String>,
    node_id: Option<String>,
    step_index: Option<i64>,
    aer_base: Option<u32>,
    aer_payload_hex: Option<String>,
    spike_indices: Option<Vec<u32>>,
    is_backward: Option<bool>,
}

#[derive(Deserialize)]
struct LoginPayload {
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct SignupPayload {
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct OidcExchangePayload {
    access_token: Option<String>,
    id_token: Option<String>,
    next: Option<String>,
}

#[derive(Deserialize)]
struct UserConfigPayload {
    config: serde_json::Value,
}

#[derive(Serialize)]
struct AuthModeResponse {
    mode: String,
    allow_signup: bool,
}

#[derive(Serialize)]
struct UiConfigResponse {
    default_orchestrator: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = Args::parse();
    apply_env_overrides(&mut args);
    let auth_mode_val = AuthMode::parse(&args.auth_mode);
    let mut users = UserStore::load(&args.users_file).await;

    if auth_mode_val == AuthMode::Local {
        if let (Some(user), Some(pass)) = (args.local_user.as_deref(), args.local_pass.as_deref()) {
            let hash = hash_password(pass)?;
            let existing = users.find_by_username_mut(user);
            match existing {
                Some(rec) => {
                    rec.password_hash = Some(hash);
                }
                None => {
                    users.users.push(UserRecord {
                        username: user.to_string(),
                        password_hash: Some(hash),
                        oidc_subject: None,
                        oidc_issuer: None,
                        email: None,
                        created_at: now_ts(),
                        config: None,
                    });
                }
            }
            let _ = users.save(&args.users_file).await;
        }
    }

    let oidc = if auth_mode_val == AuthMode::Oidc {
        let issuer = args
            .oidc_issuer
            .clone()
            .ok_or_else(|| anyhow::anyhow!("missing --oidc-issuer"))?;
        let client_id = args
            .oidc_client_id
            .clone()
            .ok_or_else(|| anyhow::anyhow!("missing --oidc-client-id"))?;
        let client_secret = args
            .oidc_client_secret
            .clone()
            .ok_or_else(|| anyhow::anyhow!("missing --oidc-client-secret"))?;
        let redirect_url = args
            .oidc_redirect_url
            .clone()
            .ok_or_else(|| anyhow::anyhow!("missing --oidc-redirect-url"))?;

        let http_client = reqwest::ClientBuilder::new()
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let provider_metadata =
            CoreProviderMetadata::discover_async(IssuerUrl::new(issuer.clone())?, &http_client)
                .await?;
        let client: OidcClient = CoreClient::from_provider_metadata(
            provider_metadata,
            ClientId::new(client_id),
            Some(ClientSecret::new(client_secret)),
        )
        .set_redirect_uri(RedirectUrl::new(redirect_url)?);

        Some(OidcConfig {
            client,
            http_client,
            issuer,
            pending: Arc::new(RwLock::new(HashMap::new())),
        })
    } else {
        None
    };

    let auth = AuthConfig {
        mode: auth_mode_val,
        users_file: args.users_file.clone(),
        allow_signup: args.allow_signup,
        session_ttl_secs: args.session_ttl_secs,
        oidc,
    };

    let state = Arc::new(AppState {
        default_orchestrator: args.orchestrator,
        auth,
        users: Arc::new(RwLock::new(users)),
        sessions: Arc::new(RwLock::new(HashMap::new())),
    });

    let api = Router::new()
        .route("/openapi.json", get(openapi_json))
        .route("/config", get(api_config))
        .route("/auth/mode", get(auth_mode_handler))
        .route("/me", get(me))
        .route("/login", post(login))
        .route("/signup", post(signup))
        .route("/logout", post(logout))
        .route("/user/config", get(get_user_config).post(set_user_config))
        .route("/status", get(status))
        .route("/snapshot", get(snapshot))
        .route("/activity", get(activity))
        .route("/export", get(export))
        .route("/aer/inject", post(aer_inject))
        .route("/aer/stream", post(aer_stream))
        .route("/update_network", post(update_network))
        .route("/control_network", post(control_network))
        .layer(DefaultBodyLimit::max(WEB_UI_GRPC_MAX_MESSAGE_BYTES))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            api_auth_middleware,
        ));

    let app = Router::new()
        .route("/", get(index))
        .route("/app.js", get(app_js))
        .route("/style.css", get(style_css))
        .route("/docs", get(docs_page))
        .route("/docs/", get(docs_page))
        .route("/docs/swagger", get(swagger_page))
        .route("/docs/swagger/", get(swagger_page))
        .route("/auth/oidc/login", get(oidc_login))
        .route("/auth/oidc/callback", get(oidc_callback))
        .route("/auth/oidc/exchange", post(oidc_exchange))
        .nest("/api", api)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(args.listen).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn index() -> impl IntoResponse {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    (headers, include_str!("../../web_ui/index.html"))
}

async fn app_js() -> impl IntoResponse {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("application/javascript; charset=utf-8"),
    );
    (headers, include_str!("../../web_ui/app.js"))
}

async fn style_css() -> impl IntoResponse {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("text/css; charset=utf-8"),
    );
    (headers, include_str!("../../web_ui/style.css"))
}

async fn docs_page() -> impl IntoResponse {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    (headers, include_str!("../../web_ui/docs.html"))
}

async fn swagger_page() -> impl IntoResponse {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    (headers, include_str!("../../web_ui/swagger.html"))
}

async fn openapi_json(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Json(build_openapi_spec(&state))
}

fn build_openapi_spec(state: &AppState) -> Value {
    let auth_mode = state.auth.mode.as_str();
    let default_orchestrator = state
        .default_orchestrator
        .clone()
        .unwrap_or_else(|| "(not configured)".to_string());
    let auth_note = format!(
        "Current runtime auth mode: `{}`. Protected endpoints require the `nm_session` cookie. \
Use `POST /api/login` (local mode) or OIDC endpoints to establish a session.",
        auth_mode
    );

    json!({
      "openapi": "3.0.3",
      "info": {
        "title": "Neuromorphic Web UI API",
        "version": "1.0.0",
        "description": format!(
          "API for orchestrator status, network control, snapshots, activity, export, and user configuration.\n\n{}\n\nDefault orchestrator configured on this server: `{}`.",
          auth_note,
          default_orchestrator
        )
      },
      "servers": [
        { "url": "/", "description": "Current web-ui origin" }
      ],
      "externalDocs": {
        "description": "Human-readable docs and usage guidance",
        "url": "/docs"
      },
      "tags": [
        { "name": "docs", "description": "API documentation endpoints" },
        { "name": "auth", "description": "Authentication and session endpoints" },
        { "name": "config", "description": "Web UI config bootstrap endpoints" },
        { "name": "user", "description": "Per-user persisted configuration" },
        { "name": "cluster", "description": "Cluster/system status and telemetry" },
        { "name": "network", "description": "Network snapshot/activity/control/update/export APIs" },
        { "name": "oidc", "description": "OIDC browser and token exchange endpoints" }
      ],
      "components": {
        "securitySchemes": {
          "cookieAuth": {
            "type": "apiKey",
            "in": "cookie",
            "name": "nm_session",
            "description": "Session cookie set by /api/login or OIDC flow."
          }
        },
        "schemas": {
          "ErrorResponse": {
            "type": "object",
            "properties": {
              "error": { "type": "string" }
            },
            "required": ["error"]
          },
          "AuthModeResponse": {
            "type": "object",
            "properties": {
              "mode": { "type": "string", "enum": ["none", "local", "oidc"] },
              "allow_signup": { "type": "boolean" }
            },
            "required": ["mode", "allow_signup"]
          },
          "UiConfigResponse": {
            "type": "object",
            "properties": {
              "default_orchestrator": { "type": "string", "nullable": true, "description": "Default gRPC orchestrator address." }
            }
          },
          "MeResponse": {
            "type": "object",
            "properties": {
              "authenticated": { "type": "boolean" },
              "mode": { "type": "string", "nullable": true },
              "username": { "type": "string", "nullable": true }
            },
            "required": ["authenticated"]
          },
          "LoginPayload": {
            "type": "object",
            "properties": {
              "username": { "type": "string" },
              "password": { "type": "string", "format": "password" }
            },
            "required": ["username", "password"]
          },
          "SignupPayload": {
            "type": "object",
            "properties": {
              "username": { "type": "string" },
              "password": { "type": "string", "format": "password" }
            },
            "required": ["username", "password"]
          },
          "LoginResponse": {
            "type": "object",
            "properties": {
              "ok": { "type": "boolean" },
              "username": { "type": "string" }
            },
            "required": ["ok", "username"]
          },
          "SignupResponse": {
            "type": "object",
            "properties": {
              "ok": { "type": "boolean" }
            },
            "required": ["ok"]
          },
          "LogoutResponse": {
            "type": "object",
            "properties": {
              "ok": { "type": "boolean" }
            },
            "required": ["ok"]
          },
          "UserConfigPayload": {
            "type": "object",
            "properties": {
              "config": { "type": "object", "additionalProperties": true }
            },
            "required": ["config"]
          },
          "UserConfigResponse": {
            "type": "object",
            "properties": {
              "config": { "type": "object", "additionalProperties": true }
            },
            "required": ["config"]
          },
          "SuccessResponse": {
            "type": "object",
            "properties": {
              "success": { "type": "boolean" }
            },
            "required": ["success"]
          },
          "NodeStatus": {
            "type": "object",
            "properties": {
              "node_id": { "type": "string" },
              "address": { "type": "string" },
              "active_networks": { "type": "array", "items": { "type": "string" } },
              "cpu_usage": { "type": "number" },
              "total_ram": { "type": "integer", "format": "uint64" },
              "available_ram": { "type": "integer", "format": "uint64" },
              "num_gpus": { "type": "integer", "format": "uint32" },
              "num_tpus": { "type": "integer", "format": "uint32" },
              "num_fpgas": { "type": "integer", "format": "uint32" },
              "capacity_score": { "type": "number" },
              "desired_dt": { "type": "number" },
              "num_neurons": { "type": "integer", "format": "uint64" },
              "redundant_neurons": { "type": "integer", "format": "uint64" },
              "current_aarnn_depth": { "type": "integer", "format": "uint32" },
              "desired_aarnn_depth": { "type": "integer", "format": "uint32" },
              "avg_step_time_ms": { "type": "number" },
              "temperature_c": { "type": "number" },
              "ga_running": { "type": "boolean" },
              "ga_generation": { "type": "integer", "format": "uint32" },
              "ga_best_fitness": { "type": "number" },
              "ga_best_config_json": { "type": "string" },
              "ga_pacing": { "type": "boolean" },
              "ga_pacing_reason": { "type": "string" },
              "ga_evaluating": { "type": "boolean" },
              "ga_eval_progress": { "type": "number" },
              "ga_total_evaluations": { "type": "integer", "format": "uint64" },
              "ga_active_eval_seed": { "type": "integer", "format": "uint64" },
              "ga_ramp_active": { "type": "boolean" },
              "ga_ramp_population": { "type": "integer", "format": "uint32" },
              "ga_ramp_worker_cap": { "type": "integer", "format": "uint32" },
              "ga_ramp_sim_time_ms": { "type": "number" },
              "ga_ramp_eval_ms": { "type": "integer", "format": "uint64" },
              "ga_ramp_eval_neurons": { "type": "integer", "format": "uint64" },
              "ga_ramp_eval_conns": { "type": "integer", "format": "uint64" },
              "comm_protocol": { "type": "string" },
              "peer_comm_protocols": {
                "type": "object",
                "additionalProperties": { "type": "string" }
              }
            },
            "required": ["node_id", "address", "active_networks"]
          },
          "NetworkDistributionEntry": {
            "type": "object",
            "properties": {
              "node_id": { "type": "string" },
              "layers": { "type": "array", "items": { "type": "integer", "format": "uint32" } },
              "layer_neuron_counts": {
                "type": "object",
                "additionalProperties": { "type": "integer", "format": "uint64" }
              }
            },
            "required": ["node_id", "layers", "layer_neuron_counts"]
          },
          "NetworkStatus": {
            "type": "object",
            "properties": {
              "network_id": { "type": "string" },
              "current_dt": { "type": "number" },
              "total_neurons": { "type": "integer", "format": "uint64" },
              "num_layers": { "type": "integer", "format": "uint32" },
              "desired_aarnn_depth": { "type": "integer", "format": "uint32" },
              "playing": { "type": "boolean" },
              "neuron_model": { "type": "string" },
              "learning_rule": { "type": "string" },
              "distribution": { "type": "array", "items": { "$ref": "#/components/schemas/NetworkDistributionEntry" } }
            },
            "required": ["network_id", "distribution"]
          },
          "StatusResponse": {
            "type": "object",
            "properties": {
              "orchestrator": { "type": "string" },
              "nodes": { "type": "array", "items": { "$ref": "#/components/schemas/NodeStatus" } },
              "networks": { "type": "array", "items": { "$ref": "#/components/schemas/NetworkStatus" } },
              "timestamp_ms": { "type": "integer", "format": "uint64" }
            },
            "required": ["orchestrator", "nodes", "networks", "timestamp_ms"]
          },
          "SnapshotResponse": {
            "type": "object",
            "properties": {
              "network_id": { "type": "string" },
              "snapshot_json": { "type": "string", "description": "Serialized network snapshot JSON." },
              "source": { "type": "string", "description": "Resolved node address used for this request." }
            },
            "required": ["network_id", "snapshot_json", "source"]
          },
          "ActivityIndices": {
            "type": "object",
            "properties": {
              "indices": { "type": "array", "items": { "type": "integer", "format": "uint32" } }
            },
            "required": ["indices"]
          },
          "ActivityResponse": {
            "type": "object",
            "properties": {
              "network_id": { "type": "string" },
              "sensory": { "$ref": "#/components/schemas/ActivityIndices" },
              "hidden": { "type": "array", "items": { "$ref": "#/components/schemas/ActivityIndices" } },
              "output": { "$ref": "#/components/schemas/ActivityIndices" },
              "source": { "type": "string" }
            },
            "required": ["network_id", "sensory", "hidden", "output", "source"]
          },
          "UpdateNetworkPayload": {
            "type": "object",
            "properties": {
              "addr": { "type": "string", "nullable": true },
              "network_id": { "type": "string" },
              "config_json": { "type": "string", "nullable": true },
              "neuron_model": { "type": "string", "nullable": true },
              "learning_rule": { "type": "string", "nullable": true }
            },
            "required": ["network_id"]
          },
          "ControlNetworkPayload": {
            "type": "object",
            "properties": {
              "addr": { "type": "string", "nullable": true },
              "network_id": { "type": "string" },
              "action": {
                "type": "string",
                "enum": ["start", "stop", "repeat", "reset", "new"]
              }
            },
            "required": ["network_id", "action"]
          },
          "AerInjectPayload": {
            "type": "object",
            "properties": {
              "addr": { "type": "string", "nullable": true, "description": "Orchestrator address; defaults to server config." },
              "network_id": { "type": "string" },
              "node_id": { "type": "string", "nullable": true, "description": "Optional specific node target." },
              "step_index": { "type": "integer", "format": "int64", "nullable": true },
              "aer_base": { "type": "integer", "format": "uint32", "nullable": true, "description": "Base address for decoding AER payload." },
              "aer_payload_hex": { "type": "string", "nullable": true, "description": "Hex-encoded AER1 payload bytes." },
              "spike_indices": { "type": "array", "items": { "type": "integer", "format": "uint32" }, "nullable": true, "description": "Fallback direct sensory spike indices." },
              "is_backward": { "type": "boolean", "nullable": true, "description": "Reserved; normally false for sensory injection." }
            },
            "required": ["network_id"]
          },
          "AerInjectResponse": {
            "type": "object",
            "properties": {
              "accepted": { "type": "integer", "format": "uint64" },
              "target": { "type": "string" },
              "network_id": { "type": "string" },
              "frames": { "type": "integer", "format": "uint64", "nullable": true },
              "mode": { "type": "string", "nullable": true }
            },
            "required": ["accepted", "target", "network_id"]
          },
          "OidcExchangePayload": {
            "type": "object",
            "properties": {
              "access_token": { "type": "string", "nullable": true },
              "id_token": { "type": "string", "nullable": true },
              "next": { "type": "string", "nullable": true }
            }
          }
        }
      },
      "paths": {
        "/api/openapi.json": {
          "get": {
            "tags": ["docs"],
            "summary": "Get OpenAPI specification",
            "operationId": "getOpenApiSpec",
            "responses": {
              "200": {
                "description": "OpenAPI v3 specification for this server."
              }
            }
          }
        },
        "/api/config": {
          "get": {
            "tags": ["config"],
            "summary": "Get web-ui bootstrap config",
            "operationId": "getUiConfig",
            "responses": {
              "200": {
                "description": "UI bootstrap configuration.",
                "content": { "application/json": { "schema": { "$ref": "#/components/schemas/UiConfigResponse" } } }
              }
            }
          }
        },
        "/api/auth/mode": {
          "get": {
            "tags": ["auth"],
            "summary": "Get auth mode",
            "operationId": "getAuthMode",
            "responses": {
              "200": {
                "description": "Authentication mode and signup capability.",
                "content": { "application/json": { "schema": { "$ref": "#/components/schemas/AuthModeResponse" } } }
              }
            }
          }
        },
        "/api/me": {
          "get": {
            "tags": ["auth"],
            "summary": "Get current session user",
            "operationId": "getCurrentUser",
            "responses": {
              "200": {
                "description": "Authenticated status.",
                "content": { "application/json": { "schema": { "$ref": "#/components/schemas/MeResponse" } } }
              },
              "401": {
                "description": "No valid session cookie.",
                "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } }
              }
            }
          }
        },
        "/api/login": {
          "post": {
            "tags": ["auth"],
            "summary": "Login (local auth mode)",
            "description": "On success sets `nm_session` cookie.",
            "operationId": "loginLocal",
            "requestBody": {
              "required": true,
              "content": {
                "application/json": {
                  "schema": { "$ref": "#/components/schemas/LoginPayload" },
                  "examples": {
                    "default": {
                      "value": { "username": "admin", "password": "change-me" }
                    }
                  }
                }
              }
            },
            "responses": {
              "200": {
                "description": "Login successful.",
                "content": { "application/json": { "schema": { "$ref": "#/components/schemas/LoginResponse" } } }
              },
              "400": { "description": "Missing credentials or local auth disabled.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "401": { "description": "Invalid credentials.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } }
            }
          }
        },
        "/api/signup": {
          "post": {
            "tags": ["auth"],
            "summary": "Signup (if enabled)",
            "operationId": "signupLocal",
            "requestBody": {
              "required": true,
              "content": {
                "application/json": {
                  "schema": { "$ref": "#/components/schemas/SignupPayload" }
                }
              }
            },
            "responses": {
              "200": { "description": "Signup successful.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/SignupResponse" } } } },
              "400": { "description": "Invalid request or local auth disabled.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "403": { "description": "Signup disabled.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "409": { "description": "User exists.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } }
            }
          }
        },
        "/api/logout": {
          "post": {
            "tags": ["auth"],
            "summary": "Logout session",
            "operationId": "logout",
            "security": [{ "cookieAuth": [] }],
            "responses": {
              "200": { "description": "Session cleared.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/LogoutResponse" } } } }
            }
          }
        },
        "/api/user/config": {
          "get": {
            "tags": ["user"],
            "summary": "Get saved user UI config",
            "operationId": "getUserConfig",
            "security": [{ "cookieAuth": [] }],
            "responses": {
              "200": { "description": "Saved config payload.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/UserConfigResponse" } } } },
              "401": { "description": "Unauthorized.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } }
            }
          },
          "post": {
            "tags": ["user"],
            "summary": "Persist user UI config",
            "operationId": "setUserConfig",
            "security": [{ "cookieAuth": [] }],
            "requestBody": {
              "required": true,
              "content": {
                "application/json": {
                  "schema": { "$ref": "#/components/schemas/UserConfigPayload" }
                }
              }
            },
            "responses": {
              "200": { "description": "Config saved.", "content": { "application/json": { "schema": { "type": "object", "properties": { "ok": { "type": "boolean" } }, "required": ["ok"] } } } },
              "401": { "description": "Unauthorized.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } }
            }
          }
        },
        "/api/status": {
          "get": {
            "tags": ["cluster"],
            "summary": "Get cluster/system status",
            "operationId": "getSystemStatus",
            "security": [{ "cookieAuth": [] }],
            "parameters": [
              {
                "name": "addr",
                "in": "query",
                "required": false,
                "description": "Optional orchestrator gRPC address; defaults to server configured value.",
                "schema": { "type": "string" }
              }
            ],
            "responses": {
              "200": { "description": "Cluster status payload.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/StatusResponse" } } } },
              "400": { "description": "Missing orchestrator address.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "401": { "description": "Unauthorized.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "503": { "description": "Unable to connect to orchestrator.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } }
            }
          }
        },
        "/api/snapshot": {
          "get": {
            "tags": ["network"],
            "summary": "Get network snapshot JSON",
            "operationId": "getNetworkSnapshot",
            "security": [{ "cookieAuth": [] }],
            "parameters": [
              { "name": "network_id", "in": "query", "required": true, "schema": { "type": "string" } },
              { "name": "addr", "in": "query", "required": false, "schema": { "type": "string" } },
              { "name": "node_id", "in": "query", "required": false, "schema": { "type": "string" }, "description": "Optional specific node in cluster to query." }
            ],
            "responses": {
              "200": { "description": "Serialized network snapshot.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/SnapshotResponse" } } } },
              "400": { "description": "Missing network_id or address.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "401": { "description": "Unauthorized.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "503": { "description": "Snapshot fetch failed.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } }
            }
          }
        },
        "/api/activity": {
          "get": {
            "tags": ["network"],
            "summary": "Get latest spike activity",
            "operationId": "getNetworkActivity",
            "security": [{ "cookieAuth": [] }],
            "parameters": [
              { "name": "network_id", "in": "query", "required": true, "schema": { "type": "string" } },
              { "name": "addr", "in": "query", "required": false, "schema": { "type": "string" } },
              { "name": "node_id", "in": "query", "required": false, "schema": { "type": "string" } }
            ],
            "responses": {
              "200": { "description": "Activity vectors for sensory/hidden/output.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ActivityResponse" } } } },
              "400": { "description": "Missing network_id or address.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "401": { "description": "Unauthorized.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "503": { "description": "Activity fetch failed.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } }
            }
          }
        },
        "/api/update_network": {
          "post": {
            "tags": ["network"],
            "summary": "Update network config/model/learning rule",
            "operationId": "updateNetworkConfig",
            "security": [{ "cookieAuth": [] }],
            "requestBody": {
              "required": true,
              "content": {
                "application/json": {
                  "schema": { "$ref": "#/components/schemas/UpdateNetworkPayload" }
                }
              }
            },
            "responses": {
              "200": { "description": "Update result.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/SuccessResponse" } } } },
              "400": { "description": "Missing orchestrator address.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "401": { "description": "Unauthorized.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "500": { "description": "Update failed.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "503": { "description": "Connect failed.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } }
            }
          }
        },
        "/api/control_network": {
          "post": {
            "tags": ["network"],
            "summary": "Send control action (start/stop/repeat/reset/new)",
            "operationId": "controlNetwork",
            "security": [{ "cookieAuth": [] }],
            "requestBody": {
              "required": true,
              "content": {
                "application/json": {
                  "schema": { "$ref": "#/components/schemas/ControlNetworkPayload" }
                }
              }
            },
            "responses": {
              "200": { "description": "Control result.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/SuccessResponse" } } } },
              "400": { "description": "Invalid action or missing address.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "401": { "description": "Unauthorized.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "500": { "description": "Update failed.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "503": { "description": "Connect failed.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } }
            }
          }
        },
        "/api/aer/inject": {
          "post": {
            "tags": ["network"],
            "summary": "Inject one AER exchange into a running network",
            "description": "Injects sensory spikes into the next simulation step. Accepts either a hex-encoded AER payload (`aer_payload_hex`) or direct `spike_indices`.",
            "operationId": "injectAerExchange",
            "security": [{ "cookieAuth": [] }],
            "requestBody": {
              "required": true,
              "content": {
                "application/json": {
                  "schema": { "$ref": "#/components/schemas/AerInjectPayload" },
                  "examples": {
                    "spikeIndices": {
                      "value": {
                        "network_id": "default",
                        "spike_indices": [0, 4, 17]
                      }
                    },
                    "aerPayloadHex": {
                      "value": {
                        "network_id": "default",
                        "aer_base": 4096,
                        "aer_payload_hex": "41455231b80b0000000000000100802001"
                      }
                    }
                  }
                }
              }
            },
            "responses": {
              "200": { "description": "AER exchange accepted.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/AerInjectResponse" } } } },
              "400": { "description": "Invalid payload or missing fields.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "401": { "description": "Unauthorized.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "503": { "description": "Target connection/stream unavailable.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } }
            }
          }
        },
        "/api/aer/stream": {
          "post": {
            "tags": ["network"],
            "summary": "Inject a stream of AER exchanges (NDJSON over HTTP)",
            "description": "Accepts newline-delimited JSON frames in request body. Each frame can contain `aer_payload_hex` or `spike_indices`. Use `Content-Type: application/x-ndjson`.",
            "operationId": "streamAerExchange",
            "security": [{ "cookieAuth": [] }],
            "parameters": [
              { "name": "network_id", "in": "query", "required": false, "schema": { "type": "string" }, "description": "Optional default network ID for all frames; if omitted each frame must include `network_id`." },
              { "name": "addr", "in": "query", "required": false, "schema": { "type": "string" } },
              { "name": "node_id", "in": "query", "required": false, "schema": { "type": "string" } },
              { "name": "step_index", "in": "query", "required": false, "schema": { "type": "integer", "format": "int64" } },
              { "name": "aer_base", "in": "query", "required": false, "schema": { "type": "integer", "format": "uint32" } },
              { "name": "is_backward", "in": "query", "required": false, "schema": { "type": "boolean" } }
            ],
            "requestBody": {
              "required": true,
              "content": {
                "application/x-ndjson": {
                  "schema": { "type": "string", "description": "NDJSON frames, one JSON object per line." },
                  "examples": {
                    "ndjson": {
                      "value": "{\"spike_indices\":[0,1,2]}\\n{\"spike_indices\":[5,9]}\\n"
                    }
                  }
                }
              }
            },
            "responses": {
              "200": { "description": "Stream accepted and forwarded.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/AerInjectResponse" } } } },
              "400": { "description": "Invalid NDJSON or missing data.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "401": { "description": "Unauthorized.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "503": { "description": "Target connection/stream unavailable.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } }
            }
          }
        },
        "/api/export": {
          "get": {
            "tags": ["network"],
            "summary": "Export network using conversion tools",
            "operationId": "exportNetwork",
            "security": [{ "cookieAuth": [] }],
            "parameters": [
              { "name": "network_id", "in": "query", "required": true, "schema": { "type": "string" } },
              { "name": "format", "in": "query", "required": true, "schema": { "type": "string", "enum": ["neuroml", "pynn", "nir", "onnx", "tflite"] } },
              { "name": "addr", "in": "query", "required": false, "schema": { "type": "string" } }
            ],
            "responses": {
              "200": {
                "description": "Exported artifact file.",
                "content": {
                  "application/octet-stream": {
                    "schema": { "type": "string", "format": "binary" }
                  }
                }
              },
              "400": { "description": "Missing/invalid parameters.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "401": { "description": "Unauthorized.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "500": { "description": "Tool or file processing failed.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "503": { "description": "Snapshot source unavailable.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } }
            }
          }
        },
        "/auth/oidc/login": {
          "get": {
            "tags": ["oidc"],
            "summary": "Start browser OIDC login",
            "operationId": "startOidcLogin",
            "responses": {
              "303": { "description": "Redirect to OIDC authorization endpoint." },
              "503": { "description": "OIDC not configured.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } }
            }
          }
        },
        "/auth/oidc/callback": {
          "get": {
            "tags": ["oidc"],
            "summary": "OIDC redirect callback handler",
            "operationId": "oidcCallback",
            "parameters": [
              { "name": "code", "in": "query", "required": false, "schema": { "type": "string" } },
              { "name": "state", "in": "query", "required": false, "schema": { "type": "string" } },
              { "name": "error", "in": "query", "required": false, "schema": { "type": "string" } },
              { "name": "error_description", "in": "query", "required": false, "schema": { "type": "string" } }
            ],
            "responses": {
              "303": { "description": "Redirect to app after successful token exchange." },
              "400": { "description": "Invalid callback payload.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } }
            }
          }
        },
        "/auth/oidc/exchange": {
          "post": {
            "tags": ["oidc"],
            "summary": "Exchange OIDC tokens for local session",
            "operationId": "oidcExchange",
            "requestBody": {
              "required": true,
              "content": {
                "application/json": {
                  "schema": { "$ref": "#/components/schemas/OidcExchangePayload" }
                }
              }
            },
            "responses": {
              "200": { "description": "Session established; cookie set." },
              "400": { "description": "Missing token payload.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "500": { "description": "OIDC validation failed.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } }
            }
          }
        }
      }
    })
}

async fn api_auth_middleware(
    State(state): State<Arc<AppState>>,
    mut req: axum::http::Request<axum::body::Body>,
    next: middleware::Next,
) -> axum::response::Response {
    if state.auth.mode == AuthMode::None {
        req.extensions_mut().insert(AuthUser {
            username: "anonymous".to_string(),
        });
        return next.run(req).await;
    }
    let path = req.uri().path();
    if matches!(
        path,
        "/api/openapi.json"
            | "/api/config"
            | "/api/auth/mode"
            | "/api/login"
            | "/api/signup"
            | "/api/me"
    ) {
        return next.run(req).await;
    }
    let jar = CookieJar::from_headers(req.headers());
    if let Some(user) = session_user(&state, &jar).await {
        req.extensions_mut().insert(AuthUser { username: user });
        return next.run(req).await;
    }
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "unauthorized" })),
    )
        .into_response()
}

async fn auth_mode_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Json(AuthModeResponse {
        mode: state.auth.mode.as_str().to_string(),
        allow_signup: state.auth.allow_signup,
    })
}

async fn api_config(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Json(UiConfigResponse {
        default_orchestrator: state.default_orchestrator.clone(),
    })
}

async fn me(State(state): State<Arc<AppState>>, jar: CookieJar) -> axum::response::Response {
    if state.auth.mode == AuthMode::None {
        return Json(json!({ "authenticated": false, "mode": "none" })).into_response();
    }
    if let Some(user) = session_user(&state, &jar).await {
        return Json(json!({ "authenticated": true, "username": user })).into_response();
    }
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "unauthorized" })),
    )
        .into_response()
}

async fn login(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Json(payload): Json<LoginPayload>,
) -> impl IntoResponse {
    if state.auth.mode != AuthMode::Local {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "local auth disabled" })),
        )
            .into_response();
    }
    let username = payload.username.trim();
    let password = payload.password.trim();
    if username.is_empty() || password.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "missing credentials" })),
        )
            .into_response();
    }
    let users = state.users.read().await;
    let user = match users.find_by_username(username) {
        Some(u) => u,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "invalid credentials" })),
            )
                .into_response();
        }
    };
    let hash = match user.password_hash.as_deref() {
        Some(h) => h,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "invalid credentials" })),
            )
                .into_response();
        }
    };
    if !verify_password(hash, password) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "invalid credentials" })),
        )
            .into_response();
    }

    let session_id = new_session_id();
    let expires_at = now_ts() + state.auth.session_ttl_secs;
    state.sessions.write().await.insert(
        session_id.clone(),
        SessionInfo {
            username: username.to_string(),
            expires_at,
        },
    );
    let cookie = session_cookie(&session_id, state.auth.session_ttl_secs);
    let jar = jar.add(cookie);
    (jar, Json(json!({ "ok": true, "username": username }))).into_response()
}

async fn signup(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SignupPayload>,
) -> impl IntoResponse {
    if state.auth.mode != AuthMode::Local {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "local auth disabled" })),
        )
            .into_response();
    }
    if !state.auth.allow_signup {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "signup disabled" })),
        )
            .into_response();
    }
    let username = payload.username.trim();
    let password = payload.password.trim();
    if username.is_empty() || password.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "missing credentials" })),
        )
            .into_response();
    }

    let mut users = state.users.write().await;
    if users.find_by_username(username).is_some() {
        return (
            StatusCode::CONFLICT,
            Json(json!({ "error": "user exists" })),
        )
            .into_response();
    }
    let hash = match hash_password(password) {
        Ok(h) => h,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };
    users.users.push(UserRecord {
        username: username.to_string(),
        password_hash: Some(hash),
        oidc_subject: None,
        oidc_issuer: None,
        email: None,
        created_at: now_ts(),
        config: None,
    });
    let _ = users.save(&state.auth.users_file).await;
    Json(json!({ "ok": true })).into_response()
}

async fn logout(State(state): State<Arc<AppState>>, jar: CookieJar) -> impl IntoResponse {
    if let Some(cookie) = jar.get("nm_session") {
        let session_id = cookie.value().to_string();
        state.sessions.write().await.remove(&session_id);
    }
    let mut expired = Cookie::new("nm_session", "");
    expired.set_path("/");
    let jar = jar.remove(expired);
    (jar, Json(json!({ "ok": true })))
}

async fn get_user_config(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
) -> axum::response::Response {
    if state.auth.mode == AuthMode::None {
        return Json(json!({ "config": {} })).into_response();
    }
    let users = state.users.read().await;
    if let Some(rec) = users.find_by_username(&user.username) {
        return Json(json!({ "config": rec.config.clone().unwrap_or_else(|| json!({})) }))
            .into_response();
    }
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": "user not found" })),
    )
        .into_response()
}

async fn set_user_config(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Json(payload): Json<UserConfigPayload>,
) -> axum::response::Response {
    if state.auth.mode == AuthMode::None {
        return Json(json!({ "ok": true })).into_response();
    }
    let mut users = state.users.write().await;
    let rec = match users.find_by_username_mut(&user.username) {
        Some(rec) => rec,
        None => {
            users.users.push(UserRecord {
                username: user.username.clone(),
                password_hash: None,
                oidc_subject: None,
                oidc_issuer: None,
                email: None,
                created_at: now_ts(),
                config: None,
            });
            users.find_by_username_mut(&user.username).unwrap()
        }
    };
    rec.config = Some(payload.config);
    let _ = users.save(&state.auth.users_file).await;
    Json(json!({ "ok": true })).into_response()
}

#[derive(Deserialize)]
struct OidcCallbackQuery {
    code: String,
    state: String,
}

async fn oidc_login(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let oidc = match state.auth.oidc.as_ref() {
        Some(oidc) => oidc.clone(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "oidc disabled" })),
            )
                .into_response();
        }
    };
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let (auth_url, csrf_state, nonce) = oidc
        .client
        .authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        )
        .add_scope(Scope::new("openid".to_string()))
        .add_scope(Scope::new("profile".to_string()))
        .add_scope(Scope::new("email".to_string()))
        .set_pkce_challenge(pkce_challenge)
        .url();
    let state_key = csrf_state.secret().to_string();
    oidc.pending.write().await.insert(
        state_key,
        OidcPending {
            nonce,
            pkce_verifier,
        },
    );
    Redirect::to(auth_url.as_str()).into_response()
}

async fn oidc_callback(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Query(query): Query<OidcCallbackQuery>,
) -> impl IntoResponse {
    let oidc = match state.auth.oidc.as_ref() {
        Some(oidc) => oidc.clone(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "oidc disabled" })),
            )
                .into_response();
        }
    };
    let pending = {
        let mut pending = oidc.pending.write().await;
        pending.remove(&query.state)
    };
    let pending = match pending {
        Some(p) => p,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid state" })),
            )
                .into_response();
        }
    };

    let token_req = match oidc
        .client
        .exchange_code(AuthorizationCode::new(query.code))
    {
        Ok(req) => req,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("token endpoint unavailable: {}", e) })),
            )
                .into_response();
        }
    };
    let token_resp = match token_req
        .set_pkce_verifier(pending.pkce_verifier)
        .request_async(&oidc.http_client)
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("token exchange failed: {}", e) })),
            )
                .into_response();
        }
    };

    let id_token = match token_resp.id_token() {
        Some(token) => token,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "missing id_token" })),
            )
                .into_response();
        }
    };
    let claims = match id_token.claims(&oidc.client.id_token_verifier(), &pending.nonce) {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("invalid id_token: {}", e) })),
            )
                .into_response();
        }
    };
    let subject = claims.subject().as_str().to_string();
    let email = claims.email().map(|e| e.as_str().to_string());

    let mut users = state.users.write().await;
    let username = if let Some(rec) = users.find_by_oidc_mut(&oidc.issuer, &subject) {
        if rec.email.is_none() {
            rec.email = email.clone();
        }
        rec.username.clone()
    } else {
        let base = email
            .as_deref()
            .and_then(|e| e.split('@').next())
            .unwrap_or("oidc");
        let username = users.ensure_unique_username(base);
        users.users.push(UserRecord {
            username: username.clone(),
            password_hash: None,
            oidc_subject: Some(subject.clone()),
            oidc_issuer: Some(oidc.issuer.clone()),
            email: email.clone(),
            created_at: now_ts(),
            config: None,
        });
        username
    };
    let _ = users.save(&state.auth.users_file).await;

    let session_id = new_session_id();
    let expires_at = now_ts() + state.auth.session_ttl_secs;
    state.sessions.write().await.insert(
        session_id.clone(),
        SessionInfo {
            username: username.clone(),
            expires_at,
        },
    );
    let cookie = session_cookie(&session_id, state.auth.session_ttl_secs);
    let jar = jar.add(cookie);
    (jar, Redirect::to("/")).into_response()
}

async fn oidc_exchange(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Form(payload): Form<OidcExchangePayload>,
) -> impl IntoResponse {
    if state.auth.mode != AuthMode::Oidc {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "oidc disabled" })),
        )
            .into_response();
    }
    let oidc = match state.auth.oidc.as_ref() {
        Some(oidc) => oidc.clone(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "oidc disabled" })),
            )
                .into_response();
        }
    };
    let next_path = safe_next_path(payload.next);

    let mut subject: Option<String> = None;
    let mut email: Option<String> = None;
    let mut preferred_username: Option<String> = None;

    if let Some(token) = payload
        .access_token
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        let access_token = AccessToken::new(token.to_string());
        let userinfo = match oidc.client.user_info(access_token, None) {
            Ok(req) => req.request_async(&oidc.http_client).await.ok(),
            Err(_) => None,
        };
        let userinfo: Option<CoreUserInfoClaims> = userinfo;
        if let Some(info) = userinfo {
            subject = Some(info.subject().as_str().to_string());
            email = info.email().map(|e| e.as_str().to_string());
            preferred_username = info.preferred_username().map(|u| u.as_str().to_string());
        }
    }

    if subject.is_none() {
        if let Some(raw) = payload
            .id_token
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
        {
            let id_token = match CoreIdToken::from_str(raw) {
                Ok(token) => token,
                Err(_) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": "invalid id_token" })),
                    )
                        .into_response();
                }
            };
            let verifier = oidc.client.id_token_verifier();
            let claims = match id_token.claims(&verifier, |_: Option<&Nonce>| Ok(())) {
                Ok(claims) => claims,
                Err(_) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": "invalid id_token" })),
                    )
                        .into_response();
                }
            };
            subject = Some(claims.subject().as_str().to_string());
            email = claims.email().map(|e| e.as_str().to_string());
            preferred_username = claims.preferred_username().map(|u| u.as_str().to_string());
        }
    }

    let subject = match subject {
        Some(subject) => subject,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "missing token claims" })),
            )
                .into_response();
        }
    };

    let mut users = state.users.write().await;
    let username = if let Some(rec) = users.find_by_oidc_mut(&oidc.issuer, &subject) {
        if rec.email.is_none() {
            rec.email = email.clone();
        }
        rec.username.clone()
    } else {
        let base = preferred_username
            .clone()
            .or_else(|| {
                email
                    .as_ref()
                    .and_then(|e| e.split('@').next().map(|s| s.to_string()))
            })
            .unwrap_or_else(|| "oidc".to_string());
        let username = users.ensure_unique_username(&base);
        users.users.push(UserRecord {
            username: username.clone(),
            password_hash: None,
            oidc_subject: Some(subject.clone()),
            oidc_issuer: Some(oidc.issuer.clone()),
            email: email.clone(),
            created_at: now_ts(),
            config: None,
        });
        username
    };
    let _ = users.save(&state.auth.users_file).await;

    let session_id = new_session_id();
    let expires_at = now_ts() + state.auth.session_ttl_secs;
    state.sessions.write().await.insert(
        session_id.clone(),
        SessionInfo {
            username: username.clone(),
            expires_at,
        },
    );
    let cookie = session_cookie(&session_id, state.auth.session_ttl_secs);
    let jar = jar.add(cookie);
    (jar, Redirect::to(&next_path)).into_response()
}

async fn session_user(state: &AppState, jar: &CookieJar) -> Option<String> {
    let session_id = jar.get("nm_session")?.value().to_string();
    let mut sessions = state.sessions.write().await;
    if let Some(info) = sessions.get(&session_id) {
        if info.expires_at > now_ts() {
            return Some(info.username.clone());
        }
    }
    sessions.remove(&session_id);
    None
}

async fn status(
    State(state): State<Arc<AppState>>,
    Query(query): Query<StatusQuery>,
) -> impl IntoResponse {
    let addr = query
        .addr
        .or_else(|| state.default_orchestrator.clone())
        .ok_or(StatusCode::BAD_REQUEST);

    let addr = match addr {
        Ok(mut addr) => {
            if !addr.starts_with("http://") && !addr.starts_with("https://") {
                addr = format!("http://{}", addr);
            }
            addr
        }
        Err(code) => {
            return (
                code,
                Json(json!({ "error": "missing orchestrator address" })),
            )
                .into_response()
        }
    };

    let mut client = match connect_cluster_client(addr.clone()).await {
        Ok(client) => client,
        Err(e) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": format!("connect failed: {}", e) })),
            )
                .into_response();
        }
    };

    let resp = match client
        .get_system_status(Request::new(StatusRequest {}))
        .await
    {
        Ok(resp) => resp.into_inner(),
        Err(e) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": format!("status failed: {}", e) })),
            )
                .into_response();
        }
    };

    let nodes = resp
        .nodes
        .iter()
        .map(|n| {
            let res = n.resources.as_ref();
            json!({
                "node_id": n.node_id,
                "address": n.address,
                "active_networks": n.active_networks,
                "cpu_usage": res.map(|r| r.cpu_usage).unwrap_or(0.0),
                "total_ram": res.map(|r| r.total_ram).unwrap_or(0),
                "available_ram": res.map(|r| r.available_ram).unwrap_or(0),
                "num_gpus": res.map(|r| r.num_gpus).unwrap_or(0),
                "num_tpus": res.map(|r| r.num_tpus).unwrap_or(0),
                "num_fpgas": res.map(|r| r.num_fpgas).unwrap_or(0),
                "capacity_score": res.map(|r| r.capacity_score).unwrap_or(0.0),
                "desired_dt": res.map(|r| r.desired_dt).unwrap_or(0.0),
                "num_neurons": res.map(|r| r.num_neurons).unwrap_or(0),
                "redundant_neurons": res.map(|r| r.redundant_neurons).unwrap_or(0),
                "current_aarnn_depth": res.map(|r| r.current_aarnn_depth).unwrap_or(0),
                "desired_aarnn_depth": res.map(|r| r.desired_aarnn_depth).unwrap_or(0),
                "avg_step_time_ms": res.map(|r| r.avg_step_time_ms).unwrap_or(0.0),
                "temperature_c": res.map(|r| r.temperature_c).unwrap_or(-1.0),
                "ga_running": res.map(|r| r.ga_running).unwrap_or(false),
                "ga_generation": res.map(|r| r.ga_generation).unwrap_or(0),
                "ga_best_fitness": res.map(|r| r.ga_best_fitness).unwrap_or(0.0),
                "ga_best_config_json": res.map(|r| r.ga_best_config_json.clone()).unwrap_or_default(),
                "ga_pacing": res.map(|r| r.ga_pacing).unwrap_or(false),
                "ga_pacing_reason": res.map(|r| r.ga_pacing_reason.clone()).unwrap_or_default(),
                "ga_evaluating": res.map(|r| r.ga_evaluating).unwrap_or(false),
                "ga_eval_progress": res.map(|r| r.ga_eval_progress).unwrap_or(0.0),
                "ga_total_evaluations": res.map(|r| r.ga_total_evaluations).unwrap_or(0),
                "ga_active_eval_seed": res.map(|r| r.ga_active_eval_seed).unwrap_or(0),
                "ga_ramp_active": res.map(|r| r.ga_ramp_active).unwrap_or(false),
                "ga_ramp_population": res.map(|r| r.ga_ramp_population).unwrap_or(0),
                "ga_ramp_worker_cap": res.map(|r| r.ga_ramp_worker_cap).unwrap_or(0),
                "ga_ramp_sim_time_ms": res.map(|r| r.ga_ramp_sim_time_ms).unwrap_or(0.0),
                "ga_ramp_eval_ms": res.map(|r| r.ga_ramp_eval_ms).unwrap_or(0),
                "ga_ramp_eval_neurons": res.map(|r| r.ga_ramp_eval_neurons).unwrap_or(0),
                "ga_ramp_eval_conns": res.map(|r| r.ga_ramp_eval_conns).unwrap_or(0),
                "comm_protocol": res.map(|r| r.comm_protocol.clone()).unwrap_or_else(|| "unknown".to_string()),
                "peer_comm_protocols": res.map(|r| r.peer_comm_protocols.clone()).unwrap_or_default(),
            })
        })
        .collect::<Vec<_>>();

    let networks = resp
        .networks
        .iter()
        .map(|n| {
            let distribution = n
                .distribution
                .iter()
                .map(|(k, v)| {
                    json!({
                        "node_id": k,
                        "layers": v.layers,
                        "layer_neuron_counts": v.layer_neuron_counts,
                    })
                })
                .collect::<Vec<_>>();
            json!({
                "network_id": n.network_id,
                "current_dt": n.current_dt,
                "total_neurons": n.total_neurons,
                "num_layers": n.num_layers,
                "desired_aarnn_depth": n.desired_aarnn_depth,
                "playing": n.playing,
                "neuron_model": n.neuron_model,
                "learning_rule": n.learning_rule,
                "distribution": distribution,
            })
        })
        .collect::<Vec<_>>();

    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    (
        StatusCode::OK,
        Json(json!({
            "orchestrator": addr,
            "nodes": nodes,
            "networks": networks,
            "timestamp_ms": timestamp_ms,
        })),
    )
        .into_response()
}

async fn snapshot(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SnapshotQuery>,
) -> impl IntoResponse {
    let Some(network_id) = query.network_id.clone() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "missing network_id" })),
        )
            .into_response();
    };
    let addr = query
        .addr
        .or_else(|| state.default_orchestrator.clone())
        .ok_or(StatusCode::BAD_REQUEST);

    let addr = match addr {
        Ok(mut addr) => {
            if !addr.starts_with("http://") && !addr.starts_with("https://") {
                addr = format!("http://{}", addr);
            }
            addr
        }
        Err(code) => {
            return (
                code,
                Json(json!({ "error": "missing orchestrator address" })),
            )
                .into_response()
        }
    };

    let target_addr =
        match resolve_network_addr(addr.clone(), &network_id, query.node_id.clone()).await {
            Ok(a) => a,
            Err(resp) => return resp.into_response(),
        };

    let mut client = match connect_cluster_client(target_addr.clone()).await {
        Ok(client) => client,
        Err(e) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": format!("connect failed: {}", e) })),
            )
                .into_response();
        }
    };

    let resp = match client
        .get_network_snapshot(Request::new(NetworkSnapshotRequest {
            network_id: network_id.clone(),
        }))
        .await
    {
        Ok(resp) => resp.into_inner(),
        Err(e) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": format!("snapshot failed: {}", e) })),
            )
                .into_response();
        }
    };

    (
        StatusCode::OK,
        Json(json!({
            "network_id": resp.network_id,
            "snapshot_json": resp.snapshot_json,
            "source": target_addr,
        })),
    )
        .into_response()
}

async fn activity(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ActivityQuery>,
) -> impl IntoResponse {
    let Some(network_id) = query.network_id.clone() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "missing network_id" })),
        )
            .into_response();
    };
    let addr = query
        .addr
        .or_else(|| state.default_orchestrator.clone())
        .ok_or(StatusCode::BAD_REQUEST);

    let addr = match addr {
        Ok(mut addr) => {
            if !addr.starts_with("http://") && !addr.starts_with("https://") {
                addr = format!("http://{}", addr);
            }
            addr
        }
        Err(code) => {
            return (
                code,
                Json(json!({ "error": "missing orchestrator address" })),
            )
                .into_response()
        }
    };

    let target_addr =
        match resolve_network_addr(addr.clone(), &network_id, query.node_id.clone()).await {
            Ok(a) => a,
            Err(resp) => return resp.into_response(),
        };

    let mut client = match connect_cluster_client(target_addr.clone()).await {
        Ok(client) => client,
        Err(e) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": format!("connect failed: {}", e) })),
            )
                .into_response();
        }
    };

    let resp = match client
        .get_network_activity(Request::new(NetworkActivityRequest {
            network_id: network_id.clone(),
        }))
        .await
    {
        Ok(resp) => resp.into_inner(),
        Err(e) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": format!("activity failed: {}", e) })),
            )
                .into_response();
        }
    };

    let sensory = resp.sensory.map(|s| s.indices).unwrap_or_default();
    let hidden: Vec<Vec<u32>> = resp.hidden.into_iter().map(|h| h.indices).collect();
    let output = resp.output.map(|o| o.indices).unwrap_or_default();

    (
        StatusCode::OK,
        Json(json!({
            "network_id": resp.network_id,
            "sensory": { "indices": sensory },
            "hidden": hidden.into_iter().map(|indices| json!({ "indices": indices })).collect::<Vec<_>>(),
            "output": { "indices": output },
            "source": target_addr,
        })),
    )
        .into_response()
}

async fn update_network(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<UpdateNetworkPayload>,
) -> impl IntoResponse {
    let addr = payload
        .addr
        .or_else(|| state.default_orchestrator.clone())
        .ok_or(StatusCode::BAD_REQUEST);

    let addr = match addr {
        Ok(mut addr) => {
            if !addr.starts_with("http://") && !addr.starts_with("https://") {
                addr = format!("http://{}", addr);
            }
            addr
        }
        Err(code) => {
            return (
                code,
                Json(json!({ "error": "missing orchestrator address" })),
            )
                .into_response()
        }
    };

    let mut client = match connect_cluster_client(addr.clone()).await {
        Ok(client) => client,
        Err(e) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": format!("connect failed: {}", e) })),
            )
                .into_response();
        }
    };

    let update = network_update_request::Update::Config(ConfigUpdate {
        config_json: payload.config_json.unwrap_or_default().as_bytes().to_vec(),
        neuron_model: payload.neuron_model.unwrap_or_default(),
        learning_rule: payload.learning_rule.unwrap_or_default(),
    });

    let resp = match client
        .update_network(Request::new(NetworkUpdateRequest {
            network_id: payload.network_id,
            update: Some(update),
        }))
        .await
    {
        Ok(resp) => resp.into_inner(),
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("update failed: {}", e) })),
            )
                .into_response();
        }
    };

    (StatusCode::OK, Json(json!({ "success": resp.success }))).into_response()
}

async fn control_network(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ControlNetworkPayload>,
) -> impl IntoResponse {
    let addr = payload
        .addr
        .or_else(|| state.default_orchestrator.clone())
        .ok_or(StatusCode::BAD_REQUEST);

    let addr = match addr {
        Ok(mut addr) => {
            if !addr.starts_with("http://") && !addr.starts_with("https://") {
                addr = format!("http://{}", addr);
            }
            addr
        }
        Err(code) => {
            return (
                code,
                Json(json!({ "error": "missing orchestrator address" })),
            )
                .into_response()
        }
    };

    let action = match payload.action.to_lowercase().as_str() {
        "start" => control_update::Action::Start,
        "stop" => control_update::Action::Stop,
        "repeat" => control_update::Action::Repeat,
        "reset" => control_update::Action::Reset,
        "new" => control_update::Action::New,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid action" })),
            )
                .into_response()
        }
    };

    let mut client = match connect_cluster_client(addr.clone()).await {
        Ok(client) => client,
        Err(e) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": format!("connect failed: {}", e) })),
            )
                .into_response();
        }
    };

    let update = network_update_request::Update::Control(ControlUpdate {
        action: action as i32,
    });

    let resp = match client
        .update_network(Request::new(NetworkUpdateRequest {
            network_id: payload.network_id,
            update: Some(update),
        }))
        .await
    {
        Ok(resp) => resp.into_inner(),
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("update failed: {}", e) })),
            )
                .into_response();
        }
    };

    (StatusCode::OK, Json(json!({ "success": resp.success }))).into_response()
}

async fn export(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ExportQuery>,
) -> impl IntoResponse {
    let Some(network_id) = query.network_id.clone() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "missing network_id" })),
        )
            .into_response();
    };
    let addr = query
        .addr
        .or_else(|| state.default_orchestrator.clone())
        .ok_or(StatusCode::BAD_REQUEST);

    let addr = match addr {
        Ok(mut addr) => {
            if !addr.starts_with("http://") && !addr.starts_with("https://") {
                addr = format!("http://{}", addr);
            }
            addr
        }
        Err(code) => {
            return (
                code,
                Json(json!({ "error": "missing orchestrator address" })),
            )
                .into_response()
        }
    };

    let target_addr = match resolve_network_addr(addr.clone(), &network_id, None).await {
        Ok(a) => a,
        Err(resp) => return resp.into_response(),
    };

    let mut client = match connect_cluster_client(target_addr.clone()).await {
        Ok(client) => client,
        Err(e) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": format!("connect failed: {}", e) })),
            )
                .into_response();
        }
    };

    let resp = match client
        .get_network_snapshot(Request::new(NetworkSnapshotRequest {
            network_id: network_id.clone(),
        }))
        .await
    {
        Ok(resp) => resp.into_inner(),
        Err(e) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": format!("snapshot failed: {}", e) })),
            )
                .into_response();
        }
    };

    let snapshot_json = resp.snapshot_json;
    let format = query.format.to_lowercase();

    let (script, arg_in, arg_out, ext) = match format.as_str() {
        "neuroml" => ("export_neuroml.py", "--in-network", "--out-neuroml", "nml"),
        "pynn" => ("export_pynn.py", "--in-network", "--out-pynn", "py"),
        "nir" => ("export_nir.py", "--in-network", "--out-nir", "nir"),
        "onnx" => ("export_onnx.py", "--in", "--out", "onnx"),
        "tflite" => ("export_tflite.py", "--in", "--out", "tflite"),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "unsupported format" })),
            )
                .into_response()
        }
    };

    let tmp_in = std::env::temp_dir().join(format!("network_{}.json", network_id));
    let tmp_out = std::env::temp_dir().join(format!("exported_{}.{}", network_id, ext));

    if let Err(e) = fs::write(&tmp_in, snapshot_json).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("write temp failed: {}", e) })),
        )
            .into_response();
    }

    let status = tokio::process::Command::new("python3")
        .arg(format!("tools/{}", script))
        .arg(arg_in)
        .arg(&tmp_in)
        .arg(arg_out)
        .arg(&tmp_out)
        .status()
        .await;

    match status {
        Ok(s) if s.success() => match fs::read(&tmp_out).await {
            Ok(content) => {
                let mut headers = axum::http::HeaderMap::new();
                headers.insert(
                    axum::http::header::CONTENT_TYPE,
                    HeaderValue::from_static("application/octet-stream"),
                );
                headers.insert(
                    axum::http::header::CONTENT_DISPOSITION,
                    HeaderValue::from_str(&format!(
                        "attachment; filename=\"exported_{}.{}\"",
                        network_id, ext
                    ))
                    .unwrap(),
                );
                (StatusCode::OK, headers, content).into_response()
            }
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("read output failed: {}", e) })),
            )
                .into_response(),
        },
        Ok(s) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("export tool failed with status {}", s) })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("failed to run export tool: {}", e) })),
        )
            .into_response(),
    }
}

type ApiError = (StatusCode, Json<serde_json::Value>);

fn normalize_target_addr(raw: &str) -> String {
    if raw.starts_with("http://") || raw.starts_with("https://") {
        raw.to_string()
    } else {
        format!("http://{}", raw)
    }
}

fn resolve_addr_or_default(
    addr: Option<String>,
    default_addr: Option<String>,
) -> Result<String, ApiError> {
    let raw = addr.or(default_addr).ok_or((
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": "missing orchestrator address" })),
    ))?;
    Ok(normalize_target_addr(&raw))
}

fn decode_hex_payload(raw: Option<String>) -> Result<Vec<u8>, ApiError> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    hex::decode(trimmed).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("invalid aer_payload_hex: {}", e) })),
        )
    })
}

fn build_aer_batch(
    network_id: String,
    step_index: i64,
    aer_base: u32,
    is_backward: bool,
    aer_payload_hex: Option<String>,
    spike_indices: Option<Vec<u32>>,
) -> Result<SpikeBatch, ApiError> {
    let aer_payload = decode_hex_payload(aer_payload_hex)?;
    let spike_indices = spike_indices.unwrap_or_default();
    if aer_payload.is_empty() && spike_indices.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "either aer_payload_hex or spike_indices must be provided" })),
        ));
    }
    Ok(SpikeBatch {
        network_id,
        layer_index: EXTERNAL_SENSORY_LAYER_INDEX,
        step_index,
        spike_indices,
        is_backward,
        aer_payload,
        aer_base,
    })
}

async fn send_aer_batches(
    target_addr: String,
    batches: Vec<SpikeBatch>,
) -> Result<usize, ApiError> {
    if batches.is_empty() {
        return Ok(0);
    }
    let mut client = connect_cluster_client(target_addr.clone())
        .await
        .map_err(|e| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": format!("connect failed: {}", e) })),
            )
        })?;

    let (tx, rx) = mpsc::channel::<SpikeBatch>(batches.len().clamp(1, 256));
    let outbound = ReceiverStream::new(rx);
    let response = client
        .stream_spikes(Request::new(outbound))
        .await
        .map_err(|e| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": format!("stream_spikes failed: {}", e) })),
            )
        })?;
    let mut inbound = response.into_inner();
    let drain = tokio::spawn(async move { while let Ok(Some(_msg)) = inbound.message().await {} });

    let mut accepted = 0usize;
    for batch in batches {
        tx.send(batch).await.map_err(|e| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": format!("stream send failed: {}", e) })),
            )
        })?;
        accepted += 1;
    }
    drop(tx);
    let _ = drain.await;
    Ok(accepted)
}

async fn aer_inject(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<AerInjectPayload>,
) -> impl IntoResponse {
    let orchestrator_addr =
        match resolve_addr_or_default(payload.addr, state.default_orchestrator.clone()) {
            Ok(addr) => addr,
            Err(err) => return err.into_response(),
        };

    let target_addr = if payload.node_id.is_some() {
        match resolve_network_addr(
            orchestrator_addr.clone(),
            &payload.network_id,
            payload.node_id.clone(),
        )
        .await
        {
            Ok(addr) => addr,
            Err(err) => return err.into_response(),
        }
    } else {
        orchestrator_addr
    };

    let batch = match build_aer_batch(
        payload.network_id.clone(),
        payload.step_index.unwrap_or(0),
        payload.aer_base.unwrap_or(0),
        payload.is_backward.unwrap_or(false),
        payload.aer_payload_hex,
        payload.spike_indices,
    ) {
        Ok(batch) => batch,
        Err(err) => return err.into_response(),
    };

    match send_aer_batches(target_addr.clone(), vec![batch]).await {
        Ok(accepted) => (
            StatusCode::OK,
            Json(json!({
                "accepted": accepted,
                "target": target_addr,
                "network_id": payload.network_id,
            })),
        )
            .into_response(),
        Err(err) => err.into_response(),
    }
}

async fn aer_stream(
    State(state): State<Arc<AppState>>,
    Query(query): Query<AerStreamQuery>,
    body: axum::body::Body,
) -> impl IntoResponse {
    let orchestrator_addr =
        match resolve_addr_or_default(query.addr, state.default_orchestrator.clone()) {
            Ok(addr) => addr,
            Err(err) => return err.into_response(),
        };

    let query_network_id = query
        .network_id
        .clone()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty());

    let mut stream = body.into_data_stream();
    let mut buffer = Vec::<u8>::new();
    let mut parsed_lines = 0usize;
    let mut queued = Vec::<SpikeBatch>::new();
    let mut stream_network_id = query_network_id.clone();

    while let Some(chunk_res) = stream.next().await {
        let chunk = match chunk_res {
            Ok(c) => c,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("failed reading stream body: {}", e) })),
                )
                    .into_response()
            }
        };
        buffer.extend_from_slice(&chunk);

        while let Some(pos) = buffer.iter().position(|b| *b == b'\n') {
            let line_bytes: Vec<u8> = buffer.drain(..=pos).collect();
            let line = match std::str::from_utf8(&line_bytes) {
                Ok(s) => s.trim(),
                Err(_) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": "stream line is not valid UTF-8" })),
                    )
                        .into_response()
                }
            };
            if line.is_empty() {
                continue;
            }
            parsed_lines += 1;
            let frame: AerStreamFrame = match serde_json::from_str(line) {
                Ok(f) => f,
                Err(e) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": format!("invalid NDJSON frame on line {}: {}", parsed_lines, e) })),
                    )
                        .into_response()
                }
            };

            let frame_network_id = frame
                .network_id
                .map(|id| id.trim().to_string())
                .filter(|id| !id.is_empty())
                .or_else(|| query_network_id.clone())
                .unwrap_or_default();
            if frame_network_id.is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("missing network_id on frame line {}", parsed_lines) })),
                )
                    .into_response();
            }
            if let Some(expected) = &stream_network_id {
                if frame_network_id != *expected {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": format!("frame line {} network_id '{}' differs from stream network_id '{}'", parsed_lines, frame_network_id, expected) })),
                    )
                        .into_response();
                }
            } else {
                stream_network_id = Some(frame_network_id.clone());
            }
            if frame.node_id.is_some() && frame.node_id != query.node_id {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("frame line {} node_id override is not supported for a single stream target", parsed_lines) })),
                )
                    .into_response();
            }
            let batch = match build_aer_batch(
                frame_network_id,
                frame.step_index.or(query.step_index).unwrap_or(0),
                frame.aer_base.or(query.aer_base).unwrap_or(0),
                frame.is_backward.or(query.is_backward).unwrap_or(false),
                frame.aer_payload_hex,
                frame.spike_indices,
            ) {
                Ok(batch) => batch,
                Err(err) => return err.into_response(),
            };
            queued.push(batch);
        }
    }

    if !buffer.is_empty() {
        let line = match std::str::from_utf8(&buffer) {
            Ok(s) => s.trim(),
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "stream tail is not valid UTF-8" })),
                )
                    .into_response()
            }
        };
        if !line.is_empty() {
            parsed_lines += 1;
            let frame: AerStreamFrame = match serde_json::from_str(line) {
                Ok(f) => f,
                Err(e) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": format!("invalid NDJSON frame on final line {}: {}", parsed_lines, e) })),
                    )
                        .into_response()
                }
            };
            let frame_network_id = frame
                .network_id
                .map(|id| id.trim().to_string())
                .filter(|id| !id.is_empty())
                .or_else(|| query_network_id.clone())
                .unwrap_or_default();
            if frame_network_id.is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("missing network_id on final line {}", parsed_lines) })),
                )
                    .into_response();
            }
            if let Some(expected) = &stream_network_id {
                if frame_network_id != *expected {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": format!("final line network_id '{}' differs from stream network_id '{}'", frame_network_id, expected) })),
                    )
                        .into_response();
                }
            } else {
                stream_network_id = Some(frame_network_id.clone());
            }
            if frame.node_id.is_some() && frame.node_id != query.node_id {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("final line node_id override is not supported for a single stream target") })),
                )
                    .into_response();
            }
            let batch = match build_aer_batch(
                frame_network_id,
                frame.step_index.or(query.step_index).unwrap_or(0),
                frame.aer_base.or(query.aer_base).unwrap_or(0),
                frame.is_backward.or(query.is_backward).unwrap_or(false),
                frame.aer_payload_hex,
                frame.spike_indices,
            ) {
                Ok(batch) => batch,
                Err(err) => return err.into_response(),
            };
            queued.push(batch);
        }
    }

    if queued.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "stream did not contain any AER frames" })),
        )
            .into_response();
    }

    let resolved_network_id = match stream_network_id {
        Some(id) => id,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "missing network_id (query or per-frame)" })),
            )
                .into_response()
        }
    };

    let target_addr = if query.node_id.is_some() {
        match resolve_network_addr(
            orchestrator_addr.clone(),
            &resolved_network_id,
            query.node_id.clone(),
        )
        .await
        {
            Ok(addr) => addr,
            Err(err) => return err.into_response(),
        }
    } else {
        orchestrator_addr
    };

    match send_aer_batches(target_addr.clone(), queued).await {
        Ok(accepted) => (
            StatusCode::OK,
            Json(json!({
                "accepted": accepted,
                "frames": parsed_lines,
                "target": target_addr,
                "network_id": resolved_network_id,
                "mode": "ndjson-stream",
            })),
        )
            .into_response(),
        Err(err) => err.into_response(),
    }
}

async fn resolve_network_addr(
    orchestrator_addr: String,
    network_id: &str,
    node_id: Option<String>,
) -> Result<String, ApiError> {
    let mut client = connect_cluster_client(orchestrator_addr.clone())
        .await
        .map_err(|e| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": format!("connect failed: {}", e) })),
            )
        })?;

    let status = client
        .get_system_status(Request::new(StatusRequest {}))
        .await
        .map_err(|e| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": format!("status failed: {}", e) })),
            )
        })?
        .into_inner();

    let normalize_addr = |addr: &str| -> String {
        if addr.starts_with("http://") || addr.starts_with("https://") {
            addr.to_string()
        } else {
            format!("http://{}", addr)
        }
    };

    let mut candidates: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut push_candidate = |addr: String| {
        let normalized = normalize_addr(&addr);
        if seen.insert(normalized.clone()) {
            candidates.push(normalized);
        }
    };

    // Honor an explicitly selected node first (if it currently advertises the network).
    if let Some(req_node_id) = node_id {
        if let Some(node) = status.nodes.iter().find(|n| {
            n.node_id == req_node_id && n.active_networks.iter().any(|id| id == network_id)
        }) {
            push_candidate(node.address.clone());
        }
    }

    // Prefer the node that currently reports the largest shard for this network.
    if let Some(net) = status.networks.iter().find(|n| n.network_id == network_id) {
        let mut ranked = net
            .distribution
            .iter()
            .map(|(nid, range)| {
                let covered_layers = range.layers.len();
                let covered_neurons = range
                    .layer_neuron_counts
                    .values()
                    .copied()
                    .map(u64::from)
                    .sum::<u64>();
                (covered_layers, covered_neurons, nid.clone())
            })
            .collect::<Vec<_>>();

        ranked.sort_by(|a, b| {
            b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)).then_with(|| a.2.cmp(&b.2))
        });

        for (_, _, nid) in ranked {
            if let Some(node) = status.nodes.iter().find(|n| {
                n.node_id == nid && n.active_networks.iter().any(|id| id == network_id)
            }) {
                push_candidate(node.address.clone());
            }
        }
    }

    // Fall back to the most capable currently-active node.
    let mut active_nodes = status
        .nodes
        .iter()
        .filter(|n| n.active_networks.iter().any(|id| id == network_id))
        .collect::<Vec<_>>();
    active_nodes.sort_by(|a, b| {
        let a_neurons = a.resources.as_ref().map(|r| r.num_neurons).unwrap_or(0);
        let b_neurons = b.resources.as_ref().map(|r| r.num_neurons).unwrap_or(0);
        let a_capacity = a.resources.as_ref().map(|r| r.capacity_score).unwrap_or(0.0);
        let b_capacity = b.resources.as_ref().map(|r| r.capacity_score).unwrap_or(0.0);

        b_neurons
            .cmp(&a_neurons)
            .then_with(|| b_capacity.partial_cmp(&a_capacity).unwrap_or(Ordering::Equal))
            .then_with(|| a.node_id.cmp(&b.node_id))
    });
    for node in active_nodes {
        push_candidate(node.address.clone());
    }

    push_candidate(orchestrator_addr);

    let target = candidates
        .first()
        .cloned()
        .ok_or_else(|| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": "no available node address for requested network" })),
            )
        })?;
    Ok(target)
}
