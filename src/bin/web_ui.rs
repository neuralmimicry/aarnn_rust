#![recursion_limit = "2048"]

use aarnn_rust::auth_store::{
    FileOidcPendingStore, FileSessionStore, OidcPendingRecord, SessionIdentityRecord, SessionRecord,
};
use aarnn_rust::central_auth::{
    CentralApiError, CentralAuthClient, CentralLoginResponse, CentralSessionResponse,
    CentralTokenActionResponse, CentralTokenClient, CentralTokenLedgerResponse,
    CentralTokenSnapshot,
};
use aarnn_rust::config::NetworkConfig;
use aarnn_rust::deployment::{default_infrastructure_roots, detect_infrastructure};
use aarnn_rust::distributed::EXTERNAL_SENSORY_LAYER_INDEX;
use aarnn_rust::distributed::proto::{
    ConfigUpdate, ControlUpdate, NetworkActivityRequest, NetworkSnapshotRequest,
    NetworkUpdateRequest, SpikeBatch, StatusRequest, control_update,
    distributed_neuromorphic_client::DistributedNeuromorphicClient, network_update_request,
};
use aarnn_rust::nmchain::{
    NmChainAccountSnapshot, NmChainClient, NmChainIdentityUpsertRequest, NmChainLedgerResponse,
    NmChainLoginObservedRequest, NmChainTokenMutationRequest,
};
use aarnn_rust::runner::decode_snapshot_with_profile_backfill;
use aarnn_rust::runtime::{RuntimeConfig, RuntimeManager};
use aarnn_rust::runtime_api::{
    WorkspaceControlAction, WorkspaceControlRequest, WorkspaceCreateRequest, WorkspaceImportRequest,
};
use aarnn_rust::service_access::{
    ResolvedServiceAccess, SERVICE_ACCESS_CONTROL, SERVICE_ACCESS_OBSERVE, SERVICE_ACCESS_REQUEST,
    SERVICE_ACCESS_USE, ServiceAccessMap, normalise_groups, resolve_service_access,
    visible_service_keys,
};
use aarnn_rust::shared_fs::{acquire_lease_with_timeout, write_json_pretty};
use aarnn_rust::spike_io::encoding::TemporalEncodingContext;
use aarnn_rust::spike_io::profiles::{SpikeIoConfig, encode_network_inputs_with};
use aarnn_rust::spike_io::transport::{
    decode_hex_payload as decode_spike_hex_payload, encode_exchange, spikes_from_transport,
};
use anyhow::Context;
use argon2::password_hash::rand_core::OsRng;
use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use axum::{
    Extension, Json, Router,
    extract::{DefaultBodyLimit, Form, Path, Query, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware,
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use clap::Parser;
use futures_util::StreamExt;
use openidconnect::{
    AccessToken, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet,
    EndpointNotSet, EndpointSet, IssuerUrl, Nonce, OAuth2TokenResponse, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, Scope, TokenResponse,
    core::{
        CoreAuthenticationFlow, CoreClient, CoreIdToken, CoreProviderMetadata, CoreUserInfoClaims,
    },
    reqwest,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::fs;
use tokio::sync::{RwLock, mpsc};
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

fn grpc_max_message_bytes() -> usize {
    const DEFAULT: usize = 512 * 1024 * 1024;
    const MIN: usize = 4 * 1024 * 1024;
    std::env::var("NM_GRPC_MAX_MESSAGE_BYTES")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|v| *v >= MIN)
        .unwrap_or(DEFAULT)
}

async fn connect_cluster_client(
    addr: String,
) -> Result<DistributedNeuromorphicClient<tonic::transport::Channel>, tonic::transport::Error> {
    let grpc_max_msg_bytes = grpc_max_message_bytes();
    let client = DistributedNeuromorphicClient::connect(addr).await?;
    Ok(client
        .max_decoding_message_size(grpc_max_msg_bytes)
        .max_encoding_message_size(grpc_max_msg_bytes))
}

fn grpc_status_to_http(status: &tonic::Status) -> StatusCode {
    match status.code() {
        tonic::Code::InvalidArgument => StatusCode::BAD_REQUEST,
        tonic::Code::NotFound => StatusCode::NOT_FOUND,
        tonic::Code::PermissionDenied => StatusCode::FORBIDDEN,
        tonic::Code::FailedPrecondition => StatusCode::CONFLICT,
        tonic::Code::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[derive(Parser, Debug)]
#[command(author, version, about = "AARNN web UI server", long_about = None)]
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

    /// Root directory for persistent runtime workspace sandboxes.
    #[arg(long, default_value = "data/runtime")]
    runtime_root: String,

    /// Scheduler tick interval for background workspace stepping.
    #[arg(long, default_value_t = 25)]
    runtime_tick_ms: u64,

    /// Local worker limit for parallel workspace execution (0 = auto).
    #[arg(long, default_value_t = 0)]
    runtime_workers: usize,

    /// Workspace autosave cadence in scheduler steps.
    #[arg(long, default_value_t = 50)]
    runtime_autosave_steps: u64,

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

    /// Optional nmchain API base URL for shared token accounting.
    #[arg(long)]
    nmchain_api_base: Option<String>,

    /// nmchain application id presented in audit records.
    #[arg(long)]
    nmchain_app_id: Option<String>,

    /// Bearer token used for nmchain API access.
    #[arg(long)]
    nmchain_api_token: Option<String>,

    /// nmchain HTTP timeout in seconds.
    #[arg(long, default_value_t = 10)]
    nmchain_timeout_secs: u64,

    /// Optional shared auth/session API base URL.
    #[arg(long)]
    central_auth_api_base: Option<String>,

    /// Shared auth/session HTTP timeout in seconds.
    #[arg(long, default_value_t = 10)]
    central_auth_timeout_secs: u64,

    /// Optional shared billing API base URL for token accounting.
    #[arg(long)]
    billing_api_base: Option<String>,

    /// Shared billing HTTP timeout in seconds.
    #[arg(long, default_value_t = 10)]
    billing_timeout_secs: u64,

    /// Shared commercial login URL used for cross-product SSO.
    #[arg(long)]
    shared_login_url: Option<String>,

    /// Shared commercial token vault URL.
    #[arg(long)]
    token_vault_url: Option<String>,

    /// Shared commercial billing dashboard URL.
    #[arg(long)]
    billing_dashboard_url: Option<String>,

    /// Shared commercial admin billing dashboard URL.
    #[arg(long)]
    billing_admin_url: Option<String>,
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
    default_runtime_user: Option<String>,
    auth: AuthConfig,
    cors: CorsConfig,
    users: Arc<RwLock<UserStore>>,
    session_store: Arc<FileSessionStore>,
    runtime: Arc<RuntimeManager>,
    chain: Option<Arc<NmChainClient>>,
    token_pricing: TokenPricing,
    commerce: CommerceConfig,
}

#[derive(Clone, Default)]
struct CorsConfig {
    allowed_origins: HashSet<String>,
}

#[derive(Clone)]
struct AuthConfig {
    mode: AuthMode,
    users_file: String,
    allow_signup: bool,
    session_ttl_secs: u64,
    oidc: Option<OidcConfig>,
    central: Option<Arc<CentralAuthClient>>,
    billing: Option<Arc<CentralTokenClient>>,
}

#[derive(Clone, Debug)]
struct TokenPricing {
    create_workspace: i64,
    import_workspace: i64,
    start_workspace: i64,
    repeat_workspace: i64,
    step_workspace: i64,
    neuron_daily_rate: i64,
}

#[derive(Clone, Debug, Default)]
struct CommerceConfig {
    shared_login_url: Option<String>,
    token_vault_url: Option<String>,
    buy_tokens_url: Option<String>,
    billing_dashboard_url: Option<String>,
    billing_admin_url: Option<String>,
}

impl TokenPricing {
    fn from_env() -> Self {
        Self {
            create_workspace: env_opt("NM_AARNN_TOKEN_CREATE_COST")
                .and_then(|value| value.parse::<i64>().ok())
                .unwrap_or(25)
                .max(0),
            import_workspace: env_opt("NM_AARNN_TOKEN_IMPORT_COST")
                .and_then(|value| value.parse::<i64>().ok())
                .unwrap_or(25)
                .max(0),
            start_workspace: env_opt("NM_AARNN_TOKEN_START_COST")
                .and_then(|value| value.parse::<i64>().ok())
                .unwrap_or(5)
                .max(0),
            repeat_workspace: env_opt("NM_AARNN_TOKEN_REPEAT_COST")
                .and_then(|value| value.parse::<i64>().ok())
                .unwrap_or(2)
                .max(0),
            step_workspace: env_opt("NM_AARNN_TOKEN_STEP_COST")
                .and_then(|value| value.parse::<i64>().ok())
                .unwrap_or(1)
                .max(0),
            neuron_daily_rate: env_opt("NM_AARNN_TOKEN_NEURON_DAILY_RATE")
                .or_else(|| env_opt("AARNN_TOKEN_NEURON_DAILY_RATE"))
                .and_then(|value| value.parse::<i64>().ok())
                .unwrap_or(1)
                .max(0),
        }
    }

    fn control_cost(&self, action: WorkspaceControlAction) -> i64 {
        match action {
            WorkspaceControlAction::Start => self.start_workspace,
            WorkspaceControlAction::Repeat => self.repeat_workspace,
            WorkspaceControlAction::Step => self.step_workspace,
            WorkspaceControlAction::Stop
            | WorkspaceControlAction::Reset
            | WorkspaceControlAction::New
            | WorkspaceControlAction::Save => 0,
        }
    }
}

fn control_action_label(action: WorkspaceControlAction) -> &'static str {
    match action {
        WorkspaceControlAction::Start => "start",
        WorkspaceControlAction::Stop => "stop",
        WorkspaceControlAction::Repeat => "repeat",
        WorkspaceControlAction::Reset => "reset",
        WorkspaceControlAction::New => "new",
        WorkspaceControlAction::Save => "save",
        WorkspaceControlAction::Step => "step",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AccessRequirement {
    service_key: &'static str,
    access_level: &'static str,
}

impl AccessRequirement {
    const fn new(service_key: &'static str, access_level: &'static str) -> Self {
        Self {
            service_key,
            access_level,
        }
    }

    const fn aarnn_request() -> Self {
        Self::new("aarnn", SERVICE_ACCESS_REQUEST)
    }

    const fn aarnn_observe() -> Self {
        Self::new("aarnn", SERVICE_ACCESS_OBSERVE)
    }

    const fn aarnn_use() -> Self {
        Self::new("aarnn", SERVICE_ACCESS_USE)
    }

    const fn aarnn_control() -> Self {
        Self::new("aarnn", SERVICE_ACCESS_CONTROL)
    }
}

fn is_public_api_path(path: &str) -> bool {
    matches!(
        path,
        "/api/openapi.json"
            | "/api/config"
            | "/api/auth/mode"
            | "/api/login"
            | "/api/signup"
            | "/api/me"
            | "/openapi.json"
            | "/config"
            | "/auth/mode"
            | "/login"
            | "/signup"
            | "/me"
    )
}

fn api_access_requirement(method: &Method, path: &str) -> Option<AccessRequirement> {
    let segments = path
        .trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    match (method.as_str(), segments.as_slice()) {
        (_, ["api", "logout"]) => None,
        ("GET", ["api", "tokens"]) => Some(AccessRequirement::aarnn_request()),
        ("GET", ["api", "tokens", "ledger"]) => Some(AccessRequirement::aarnn_request()),
        ("GET", ["api", "user", "config"]) => Some(AccessRequirement::aarnn_request()),
        ("POST", ["api", "user", "config"]) => Some(AccessRequirement::aarnn_request()),
        ("GET", ["api", "runtime", "status"]) => Some(AccessRequirement::aarnn_observe()),
        ("GET", ["api", "runtime", "workspaces"]) => Some(AccessRequirement::aarnn_observe()),
        ("POST", ["api", "runtime", "workspaces"]) => Some(AccessRequirement::aarnn_use()),
        ("GET", ["api", "runtime", "workspaces", _]) => Some(AccessRequirement::aarnn_observe()),
        ("DELETE", ["api", "runtime", "workspaces", _]) => Some(AccessRequirement::aarnn_control()),
        ("GET", ["api", "runtime", "workspaces", _, "snapshot"]) => {
            Some(AccessRequirement::aarnn_observe())
        }
        ("GET", ["api", "runtime", "workspaces", _, "activity"]) => {
            Some(AccessRequirement::aarnn_observe())
        }
        ("POST", ["api", "runtime", "workspaces", _, "control"]) => {
            Some(AccessRequirement::aarnn_use())
        }
        ("POST", ["api", "runtime", "workspaces", _, "import"]) => {
            Some(AccessRequirement::aarnn_use())
        }
        ("GET", ["api", "runtime", "workspaces", _, "export"]) => {
            Some(AccessRequirement::aarnn_observe())
        }
        ("GET", ["api", "status"]) => Some(AccessRequirement::aarnn_observe()),
        ("GET", ["api", "snapshot"]) => Some(AccessRequirement::aarnn_observe()),
        ("GET", ["api", "activity"]) => Some(AccessRequirement::aarnn_observe()),
        ("GET", ["api", "export"]) => Some(AccessRequirement::aarnn_observe()),
        ("POST", ["api", "aer", "inject"]) => Some(AccessRequirement::aarnn_use()),
        ("POST", ["api", "aer", "stream"]) => Some(AccessRequirement::aarnn_use()),
        ("POST", ["api", "llm", "mirror"]) => Some(AccessRequirement::aarnn_use()),
        ("POST", ["api", "update_network"]) => Some(AccessRequirement::aarnn_use()),
        ("POST", ["api", "control_network"]) => Some(AccessRequirement::aarnn_use()),
        _ => None,
    }
}

fn workspace_control_requirement(action: WorkspaceControlAction) -> AccessRequirement {
    match action {
        WorkspaceControlAction::Stop
        | WorkspaceControlAction::Reset
        | WorkspaceControlAction::New => AccessRequirement::aarnn_control(),
        WorkspaceControlAction::Start
        | WorkspaceControlAction::Repeat
        | WorkspaceControlAction::Save
        | WorkspaceControlAction::Step => AccessRequirement::aarnn_use(),
    }
}

fn insufficient_service_access_response(
    user: &AuthUser,
    requirement: AccessRequirement,
) -> Response {
    let current = user.service_access_for(requirement.service_key);
    (
        StatusCode::FORBIDDEN,
        Json(json!({
            "error": "forbidden",
            "details": format!(
                "This operation requires {}:{} authorisation.",
                requirement.service_key,
                requirement.access_level
            ),
            "service_key": requirement.service_key,
            "required_access_level": requirement.access_level,
            "access_level": current.access_level,
            "visible_access_level": current.visible_access_level,
            "visible": current.visible,
        })),
    )
        .into_response()
}

const DEFAULT_SHARED_LOGIN_URL: &str = "https://neuralmimicry.ai/login";
const DEFAULT_TOKEN_VAULT_URL: &str = "https://neuralmimicry.ai/tokens";
const DEFAULT_BILLING_DASHBOARD_URL: &str = "https://neuralmimicry.ai/billing";
const DEFAULT_BILLING_ADMIN_URL: &str = "https://neuralmimicry.ai/billing/admin";
const LOCAL_SERVICE_ACCESS_OVERRIDES: &[(&str, &str)] = &[("aarnn", SERVICE_ACCESS_CONTROL)];

fn append_query_param(url: &str, key: &str, value: &str) -> String {
    let separator = if url.contains('?') {
        if url.ends_with('?') || url.ends_with('&') {
            ""
        } else {
            "&"
        }
    } else {
        "?"
    };
    format!("{url}{separator}{key}={value}")
}

fn env_opt(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn parse_env_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn env_bool(keys: &[&str]) -> Option<bool> {
    keys.iter()
        .find_map(|key| env_opt(key).and_then(|value| parse_env_bool(&value)))
}

fn normalize_origin(value: &str) -> Option<String> {
    let normalized = value.trim().trim_end_matches('/');
    if normalized.is_empty() {
        None
    } else {
        Some(normalized.to_string())
    }
}

fn env_list(keys: &[&str]) -> Vec<String> {
    let mut values = Vec::new();
    for key in keys {
        if let Some(raw) = env_opt(key) {
            values.extend(raw.split(',').filter_map(normalize_origin));
        }
    }
    values.sort();
    values.dedup();
    values
}

fn cors_allowed_origins_from_env() -> HashSet<String> {
    env_list(&["AARNN_CORS_ORIGINS", "NM_CORS_ORIGINS"])
        .into_iter()
        .collect()
}

fn cors_enabled_path(path: &str) -> bool {
    matches!(
        path,
        "/api/auth/mode" | "/auth/oidc/exchange" | "/auth/access/exchange"
    )
}

fn allowed_cors_origin(state: &AppState, headers: &HeaderMap) -> Option<String> {
    if state.cors.allowed_origins.is_empty() {
        return None;
    }
    let origin = headers.get(header::ORIGIN)?.to_str().ok()?;
    let normalized = normalize_origin(origin)?;
    state
        .cors
        .allowed_origins
        .contains(&normalized)
        .then_some(normalized)
}

fn add_vary(headers: &mut HeaderMap, value: &'static str) {
    let current = headers
        .get(header::VARY)
        .and_then(|existing| existing.to_str().ok())
        .unwrap_or("");
    if current
        .split(',')
        .any(|item| item.trim().eq_ignore_ascii_case(value))
    {
        return;
    }
    let merged = if current.is_empty() {
        value.to_string()
    } else {
        format!("{}, {}", current, value)
    };
    if let Ok(header_value) = HeaderValue::from_str(&merged) {
        headers.insert(header::VARY, header_value);
    }
}

fn apply_cors_headers(headers: &mut HeaderMap, origin: &str) {
    if let Ok(value) = HeaderValue::from_str(origin) {
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, value);
    }
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
        HeaderValue::from_static("true"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("Content-Type, Authorization, X-Requested-With"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, POST, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_MAX_AGE,
        HeaderValue::from_static("600"),
    );
    add_vary(headers, "Origin");
    add_vary(headers, "Access-Control-Request-Method");
    add_vary(headers, "Access-Control-Request-Headers");
}

fn apply_env_overrides(args: &mut Args) {
    if args.runtime_root.trim() == "data/runtime" {
        if let Some(runtime_root) =
            env_opt("NM_WEB_UI_RUNTIME_ROOT").or_else(|| env_opt("NM_RUNTIME_ROOT"))
        {
            args.runtime_root = runtime_root;
        }
    }
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
    if args.nmchain_api_base.is_none() {
        args.nmchain_api_base =
            env_opt("NMCHAIN_API_BASE").or_else(|| env_opt("NM_AARNN_CHAIN_API_BASE"));
    }
    if args.nmchain_app_id.is_none() {
        args.nmchain_app_id = env_opt("NMCHAIN_APP_ID").or_else(|| Some("aarnn".to_string()));
    }
    if args.nmchain_api_token.is_none() {
        args.nmchain_api_token =
            env_opt("NMCHAIN_API_TOKEN").or_else(|| env_opt("NM_AARNN_CHAIN_API_TOKEN"));
    }
    if args.nmchain_timeout_secs == 10 {
        if let Some(timeout) = env_opt("NMCHAIN_TIMEOUT")
            .or_else(|| env_opt("NM_AARNN_CHAIN_TIMEOUT"))
            .and_then(|value| value.parse::<u64>().ok())
        {
            args.nmchain_timeout_secs = timeout.max(1);
        }
    }
    if args.central_auth_api_base.is_none() {
        args.central_auth_api_base =
            env_opt("AARNN_CENTRAL_AUTH_API_BASE").or_else(|| env_opt("NM_CENTRAL_AUTH_API_BASE"));
    }
    if args.central_auth_timeout_secs == 10 {
        if let Some(timeout) = env_opt("AARNN_CENTRAL_AUTH_TIMEOUT_SECS")
            .or_else(|| env_opt("NM_CENTRAL_AUTH_TIMEOUT_SECS"))
            .and_then(|value| value.parse::<u64>().ok())
        {
            args.central_auth_timeout_secs = timeout.max(1);
        }
    }
    if args.billing_api_base.is_none() {
        args.billing_api_base = env_opt("AARNN_BILLING_API_BASE")
            .or_else(|| env_opt("NM_BILLING_API_BASE"))
            .or_else(|| args.central_auth_api_base.clone());
    }
    if args.billing_timeout_secs == 10 {
        if let Some(timeout) = env_opt("AARNN_BILLING_TIMEOUT_SECS")
            .or_else(|| env_opt("NM_BILLING_TIMEOUT_SECS"))
            .and_then(|value| value.parse::<u64>().ok())
        {
            args.billing_timeout_secs = timeout.max(1);
        } else if args.central_auth_timeout_secs != 10 {
            args.billing_timeout_secs = args.central_auth_timeout_secs;
        }
    }
    if args.shared_login_url.is_none() {
        args.shared_login_url = env_opt("AARNN_SHARED_LOGIN_URL")
            .or_else(|| env_opt("NM_SHARED_LOGIN_URL"))
            .or_else(|| Some(DEFAULT_SHARED_LOGIN_URL.to_string()));
    }
    if args.token_vault_url.is_none() {
        args.token_vault_url = env_opt("AARNN_TOKEN_VAULT_URL")
            .or_else(|| env_opt("NM_TOKEN_VAULT_URL"))
            .or_else(|| Some(DEFAULT_TOKEN_VAULT_URL.to_string()));
    }
    if args.billing_dashboard_url.is_none() {
        args.billing_dashboard_url = env_opt("AARNN_BILLING_DASHBOARD_URL")
            .or_else(|| env_opt("NM_BILLING_DASHBOARD_URL"))
            .or_else(|| Some(DEFAULT_BILLING_DASHBOARD_URL.to_string()));
    }
    if args.billing_admin_url.is_none() {
        args.billing_admin_url = env_opt("AARNN_BILLING_ADMIN_URL")
            .or_else(|| env_opt("NM_BILLING_ADMIN_URL"))
            .or_else(|| Some(DEFAULT_BILLING_ADMIN_URL.to_string()));
    }

    if args.auth_mode.trim().eq_ignore_ascii_case("none") {
        if let Some(mode) = env_opt("NM_AUTH_MODE").or_else(|| env_opt("NM_OIDC_AUTH_MODE")) {
            args.auth_mode = mode;
        } else if args.oidc_issuer.is_some() {
            args.auth_mode = "oidc".to_string();
        }
    }

    if args.orchestrator.is_none() || args.runtime_root.trim() == "data/runtime" {
        let roots = default_infrastructure_roots();
        if !roots.is_empty() {
            if let Ok(infra) = detect_infrastructure(&roots) {
                if args.orchestrator.is_none() {
                    args.orchestrator = infra.recommended_orchestrator_addr();
                }
                if args.runtime_root.trim() == "data/runtime" {
                    if let Some(runtime_root) = infra.runtime_root {
                        args.runtime_root = runtime_root;
                    }
                }
            }
        }
    }
}

async fn cors_middleware(
    State(state): State<Arc<AppState>>,
    req: axum::http::Request<axum::body::Body>,
    next: middleware::Next,
) -> Response {
    let path = req.uri().path().to_string();
    if !cors_enabled_path(&path) {
        return next.run(req).await;
    }

    let allowed_origin = allowed_cors_origin(&state, req.headers());
    if req.method() == Method::OPTIONS {
        if let Some(origin) = allowed_origin {
            let mut response = StatusCode::NO_CONTENT.into_response();
            apply_cors_headers(response.headers_mut(), &origin);
            return response;
        }
        return next.run(req).await;
    }

    let mut response = next.run(req).await;
    if let Some(origin) = allowed_origin {
        apply_cors_headers(response.headers_mut(), &origin);
    }
    response
}

#[derive(Clone)]
struct OidcConfig {
    client: OidcClient,
    http_client: reqwest::Client,
    issuer: String,
    pending: Arc<FileOidcPendingStore>,
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

#[derive(Clone, Default, Serialize, Deserialize)]
struct UserStore {
    users: Vec<UserRecord>,
}

#[derive(Clone, Default)]
struct AuthUser {
    username: String,
    access_token: Option<String>,
    role: String,
    groups: Vec<String>,
    email: Option<String>,
    active_team: Option<Value>,
    team_count: i64,
    pending_invitation_count: i64,
    is_admin: bool,
    service_access: ServiceAccessMap,
}

impl AuthUser {
    fn local(username: impl Into<String>) -> Self {
        Self::local_with_email(username, None)
    }

    fn local_with_email(username: impl Into<String>, email: Option<String>) -> Self {
        build_auth_user(
            username.into(),
            None,
            Some("user".to_string()),
            Vec::new(),
            email,
            None,
            Some(0),
            Some(0),
            Some(false),
            None,
            LOCAL_SERVICE_ACCESS_OVERRIDES,
        )
    }

    fn from_central_login(
        response: &CentralLoginResponse,
        fallback_username: &str,
        access_token: Option<String>,
    ) -> Option<Self> {
        let username = central_login_username(response, fallback_username)?;
        Some(build_auth_user(
            username,
            access_token,
            response.role.clone(),
            response.groups.clone(),
            response.email.clone(),
            response.active_team.clone(),
            response.team_count,
            response.pending_invitation_count,
            response.is_admin,
            Some(response.service_access.clone()),
            &[],
        ))
    }

    fn from_central_session(
        response: &CentralSessionResponse,
        access_token: Option<String>,
    ) -> Option<Self> {
        let username = central_session_username(response)?;
        Some(build_auth_user(
            username,
            access_token,
            response.role.clone(),
            response.groups.clone(),
            response.email.clone(),
            response.active_team.clone(),
            response.team_count,
            response.pending_invitation_count,
            response.is_admin,
            Some(response.service_access.clone()),
            &[],
        ))
    }

    fn from_session_record(record: SessionRecord) -> Self {
        let SessionRecord {
            username,
            access_token,
            identity,
            ..
        } = record;
        let service_access = if identity.service_access.is_empty() {
            None
        } else {
            Some(json!(identity.service_access))
        };
        let local_overrides = if access_token.is_none() {
            LOCAL_SERVICE_ACCESS_OVERRIDES
        } else {
            &[]
        };
        build_auth_user(
            username,
            access_token,
            identity.role,
            identity.groups,
            identity.email,
            identity.active_team,
            identity.team_count,
            identity.pending_invitation_count,
            identity.is_admin,
            service_access,
            local_overrides,
        )
    }

    fn session_identity(&self) -> SessionIdentityRecord {
        SessionIdentityRecord {
            role: Some(self.role.clone()),
            groups: self.groups.clone(),
            email: self.email.clone(),
            active_team: self.active_team.clone(),
            team_count: Some(self.team_count),
            pending_invitation_count: Some(self.pending_invitation_count),
            is_admin: Some(self.is_admin),
            service_access: self.service_access.clone(),
        }
    }

    fn visible_services(&self) -> Vec<String> {
        visible_service_keys(&self.service_access)
    }

    fn service_access_for(&self, service_key: &str) -> ResolvedServiceAccess {
        self.service_access
            .get(service_key)
            .cloned()
            .unwrap_or_else(|| ResolvedServiceAccess::none(service_key.to_string()))
    }

    fn can_access(&self, requirement: AccessRequirement) -> bool {
        if requirement.service_key.is_empty() {
            return true;
        }
        let access = self.service_access_for(requirement.service_key);
        match requirement.access_level {
            SERVICE_ACCESS_REQUEST => access.can_request,
            SERVICE_ACCESS_OBSERVE => access.can_observe,
            SERVICE_ACCESS_USE => access.can_use,
            SERVICE_ACCESS_CONTROL => access.can_control,
            _ => false,
        }
    }
}

impl UserStore {
    async fn load(path: &str) -> Self {
        match fs::read_to_string(path).await {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
            Err(_) => UserStore::default(),
        }
    }

    async fn save(&self, path: &str) -> anyhow::Result<()> {
        let path = PathBuf::from(path);
        let snapshot = self.clone();
        tokio::task::spawn_blocking(move || write_json_pretty(&path, &snapshot))
            .await
            .map_err(|err| anyhow::anyhow!("user store save task failed: {}", err))??;
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

fn users_lock_path(path: &str) -> PathBuf {
    PathBuf::from(format!("{}.lock", path))
}

async fn load_users_fresh(state: &AppState) -> UserStore {
    let users = UserStore::load(&state.auth.users_file).await;
    *state.users.write().await = users.clone();
    users
}

async fn modify_users<T, F>(state: &AppState, op: F) -> anyhow::Result<T>
where
    F: FnOnce(&mut UserStore) -> anyhow::Result<T>,
{
    let _lease = acquire_lease_with_timeout(
        users_lock_path(&state.auth.users_file),
        Duration::from_secs(5),
        Duration::from_millis(25),
    )
    .await?;
    let mut users = UserStore::load(&state.auth.users_file).await;
    let result = op(&mut users)?;
    users.save(&state.auth.users_file).await?;
    *state.users.write().await = users;
    Ok(result)
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

fn extract_bearer_token(headers: &HeaderMap) -> Option<String> {
    let raw = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)?;
    if let Some(token) = raw.strip_prefix("Bearer ") {
        return Some(token.trim().to_string());
    }
    if let Some(token) = raw.strip_prefix("bearer ") {
        return Some(token.trim().to_string());
    }
    None
}

fn central_login_username(response: &CentralLoginResponse, fallback: &str) -> Option<String> {
    let candidate = response
        .user
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback.trim());
    (!candidate.is_empty()).then(|| candidate.to_string())
}

fn central_session_username(response: &CentralSessionResponse) -> Option<String> {
    response
        .user
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn normalize_optional_string(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn json_value_to_nonempty_string(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(value)) => normalize_optional_string(Some(value.as_str())),
        Some(Value::Number(value)) => Some(value.to_string()),
        _ => None,
    }
}

fn normalize_identity_role(value: Option<&str>) -> String {
    normalize_optional_string(value)
        .map(|role| role.to_lowercase())
        .unwrap_or_else(|| "user".to_string())
}

fn normalize_identity_groups(values: &[String], role: &str) -> Vec<String> {
    normalise_groups(values, role)
}

fn normalize_identity_active_team(value: Option<Value>) -> Option<Value> {
    match value {
        Some(Value::Object(mut payload)) => {
            let team_id = json_value_to_nonempty_string(payload.get("team_id"))
                .or_else(|| json_value_to_nonempty_string(payload.get("id")));
            let Some(team_id) = team_id else {
                return None;
            };
            payload.insert("team_id".to_string(), Value::String(team_id));
            Some(Value::Object(payload))
        }
        Some(Value::String(team_id)) => {
            let team_id = team_id.trim();
            (!team_id.is_empty()).then(|| json!({ "team_id": team_id }))
        }
        _ => None,
    }
}

fn normalize_identity_count(value: Option<i64>, default: i64) -> i64 {
    value.unwrap_or(default).max(0)
}

fn build_auth_user(
    username: String,
    access_token: Option<String>,
    role: Option<String>,
    groups: Vec<String>,
    email: Option<String>,
    active_team: Option<Value>,
    team_count: Option<i64>,
    pending_invitation_count: Option<i64>,
    is_admin: Option<bool>,
    raw_service_access: Option<Value>,
    override_authenticated_grants: &[(&str, &str)],
) -> AuthUser {
    let username = username.trim().to_string();
    let role = normalize_identity_role(role.as_deref());
    let groups = normalize_identity_groups(&groups, &role);
    let active_team = normalize_identity_active_team(active_team);
    let team_count =
        normalize_identity_count(team_count, if active_team.is_some() { 1 } else { 0 });
    let pending_invitation_count = normalize_identity_count(pending_invitation_count, 0);
    let is_admin =
        is_admin.unwrap_or_else(|| role == "admin" || groups.iter().any(|group| group == "admin"));
    let service_access = resolve_service_access(
        raw_service_access.as_ref().filter(|value| !value.is_null()),
        true,
        &role,
        &groups,
        is_admin,
        override_authenticated_grants,
    );
    AuthUser {
        username,
        access_token: normalize_optional_string(access_token.as_deref()),
        role,
        groups,
        email: normalize_optional_string(email.as_deref()),
        active_team,
        team_count,
        pending_invitation_count,
        is_admin,
        service_access,
    }
}

fn auth_identity_payload(user: &AuthUser) -> Value {
    json!({
        "user": user.username.clone(),
        "username": user.username.clone(),
        "role": user.role.clone(),
        "groups": user.groups.clone(),
        "email": user.email.clone(),
        "active_team": user.active_team.clone(),
        "team_count": user.team_count,
        "pending_invitation_count": user.pending_invitation_count,
        "is_admin": user.is_admin,
        "service_access": user.service_access.clone(),
        "visible_services": user.visible_services(),
    })
}

fn authenticated_identity_payload(user: &AuthUser) -> Value {
    let mut payload = auth_identity_payload(user);
    if let Value::Object(map) = &mut payload {
        map.insert("authenticated".to_string(), Value::Bool(true));
    }
    payload
}

fn login_success_payload(user: &AuthUser) -> Value {
    let mut payload = authenticated_identity_payload(user);
    if let Value::Object(map) = &mut payload {
        map.insert("ok".to_string(), Value::Bool(true));
        if let Some(access_token) = user.access_token.as_ref() {
            map.insert(
                "access_token".to_string(),
                Value::String(access_token.clone()),
            );
        }
    }
    payload
}

async fn store_browser_session(
    state: &AppState,
    jar: CookieJar,
    user: &AuthUser,
) -> anyhow::Result<(CookieJar, String)> {
    let session_id = new_session_id();
    let expires_at = now_ts() + state.auth.session_ttl_secs;
    state
        .session_store
        .put(
            &session_id,
            &SessionRecord {
                username: user.username.clone(),
                expires_at,
                access_token: user.access_token.clone(),
                identity: user.session_identity(),
            },
        )
        .await?;
    let cookie = session_cookie(&session_id, state.auth.session_ttl_secs);
    Ok((jar.add(cookie), session_id))
}

#[derive(Deserialize)]
struct StatusQuery {
    addr: Option<String>,
}

#[derive(Deserialize)]
struct TokenLedgerQuery {
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct SnapshotQuery {
    addr: Option<String>,
    network_id: Option<String>,
    node_id: Option<String>,
}

#[derive(Deserialize, Default)]
struct RuntimeWorkspaceSnapshotQuery {
    owner: Option<String>,
    if_saved_after_ms: Option<u64>,
}

#[derive(Deserialize, Default)]
struct RuntimeWorkspaceOwnerQuery {
    owner: Option<String>,
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
    owner: Option<String>,
    format: String,
}

#[derive(Deserialize)]
struct AerInjectPayload {
    addr: Option<String>,
    network_id: String,
    node_id: Option<String>,
    step_index: Option<i64>,
    time_ms: Option<f32>,
    dt_ms: Option<f32>,
    aer_base: Option<u32>,
    aer_payload_hex: Option<String>,
    spike_indices: Option<Vec<u32>>,
    input_values: Option<Vec<f32>>,
    spike_io: Option<SpikeIoConfig>,
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
    time_ms: Option<f32>,
    dt_ms: Option<f32>,
    aer_base: Option<u32>,
    aer_payload_hex: Option<String>,
    spike_indices: Option<Vec<u32>>,
    input_values: Option<Vec<f32>>,
    spike_io: Option<SpikeIoConfig>,
    is_backward: Option<bool>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum LlmMirrorDirection {
    Input,
    Output,
}

#[derive(Deserialize)]
struct LlmMirrorPayload {
    request_id: String,
    conversation_id: String,
    workflow: String,
    role: String,
    direction: LlmMirrorDirection,
    provider: Option<String>,
    model: Option<String>,
    request_category: Option<String>,
    system: Option<String>,
    prompt_text: Option<String>,
    text: String,
    #[serde(default)]
    message_roles: Vec<String>,
    aer_base: u32,
    output_base: u32,
    aer_payload_hex: String,
    #[serde(default)]
    sensory_spikes: Vec<u8>,
    network_id: Option<String>,
    node_id: Option<String>,
    #[serde(default)]
    request_candidate_reply: bool,
}

#[derive(Serialize)]
struct LlmMirrorCandidateResponse {
    reply_text: Option<String>,
    confidence: Option<f64>,
    usable: bool,
    source: Option<String>,
    output_spike_indices: Vec<u32>,
    output_aer_payload_hex: Option<String>,
}

#[derive(Serialize)]
struct LlmMirrorStimulusResponse {
    attempted: bool,
    accepted_batches: usize,
    target: Option<String>,
    network_id: Option<String>,
    error: Option<String>,
}

#[derive(Serialize)]
struct LlmMirrorResponseBody {
    accepted: bool,
    request_id: String,
    conversation_id: String,
    direction: LlmMirrorDirection,
    text_chars: usize,
    spike_count: usize,
    aer_payload_hex: String,
    candidate: Option<LlmMirrorCandidateResponse>,
    stimulation: Option<LlmMirrorStimulusResponse>,
}

#[derive(Serialize)]
struct LlmMirrorRecord {
    recorded_at: u64,
    actor: String,
    actor_role: String,
    actor_groups: Vec<String>,
    request_id: String,
    conversation_id: String,
    workflow: String,
    role: String,
    direction: LlmMirrorDirection,
    provider: Option<String>,
    model: Option<String>,
    request_category: Option<String>,
    system: Option<String>,
    prompt_text: Option<String>,
    text: String,
    message_roles: Vec<String>,
    aer_base: u32,
    output_base: u32,
    aer_payload_hex: String,
    sensory_spikes: Vec<u8>,
    stimulation: LlmMirrorStimulusResponse,
    candidate: Option<LlmMirrorCandidateResponse>,
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
struct AccessExchangePayload {
    access_token: Option<String>,
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
    central_auth: bool,
}

#[derive(Serialize)]
struct UiConfigResponse {
    default_orchestrator: Option<String>,
    default_runtime_user: Option<String>,
    shared_login_url: Option<String>,
    token_vault_url: Option<String>,
    buy_tokens_url: Option<String>,
    billing_dashboard_url: Option<String>,
    billing_admin_url: Option<String>,
    neuron_daily_rate: i64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = Args::parse();
    apply_env_overrides(&mut args);
    let auth_mode_val = AuthMode::parse(&args.auth_mode);
    let central_auth = match args.central_auth_api_base.clone() {
        Some(base_url) if !base_url.trim().is_empty() => Some(Arc::new(CentralAuthClient::new(
            base_url,
            Duration::from_secs(args.central_auth_timeout_secs.max(1)),
        )?)),
        _ => None,
    };
    let billing = match args.billing_api_base.clone() {
        Some(base_url) if !base_url.trim().is_empty() => Some(Arc::new(CentralTokenClient::new(
            base_url,
            Duration::from_secs(args.billing_timeout_secs.max(1)),
        )?)),
        _ => None,
    };
    let central_auth_enabled = central_auth.is_some();
    let auth_root = std::path::Path::new(&args.users_file)
        .parent()
        .map(|path| path.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("data/auth"));
    let session_store = Arc::new(FileSessionStore::new(auth_root.join("sessions")));
    let oidc_pending_store = Arc::new(FileOidcPendingStore::new(auth_root.join("oidc-pending")));
    let users_lock = acquire_lease_with_timeout(
        users_lock_path(&args.users_file),
        Duration::from_secs(5),
        Duration::from_millis(25),
    )
    .await?;
    let mut users = UserStore::load(&args.users_file).await;

    if auth_mode_val == AuthMode::Local && !central_auth_enabled {
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
    drop(users_lock);

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
            pending: oidc_pending_store,
        })
    } else {
        None
    };

    let auth = AuthConfig {
        mode: auth_mode_val,
        users_file: args.users_file.clone(),
        allow_signup: args.allow_signup && !central_auth_enabled,
        session_ttl_secs: args.session_ttl_secs,
        oidc,
        central: central_auth,
        billing,
    };

    let chain = match args.nmchain_api_base.clone() {
        Some(base_url) if !base_url.trim().is_empty() => Some(Arc::new(NmChainClient::new(
            base_url,
            args.nmchain_app_id
                .clone()
                .unwrap_or_else(|| "aarnn".to_string()),
            args.nmchain_api_token.clone(),
            Duration::from_secs(args.nmchain_timeout_secs.max(1)),
        )?)),
        _ => None,
    };
    let token_pricing = TokenPricing::from_env();
    let commerce = CommerceConfig {
        shared_login_url: args.shared_login_url.clone(),
        token_vault_url: args.token_vault_url.clone(),
        buy_tokens_url: args
            .token_vault_url
            .as_deref()
            .map(|url| append_query_param(url, "action", "add")),
        billing_dashboard_url: args.billing_dashboard_url.clone(),
        billing_admin_url: args.billing_admin_url.clone(),
    };
    let cors = CorsConfig {
        allowed_origins: cors_allowed_origins_from_env(),
    };

    let runtime = RuntimeManager::new(RuntimeConfig {
        root_dir: std::path::PathBuf::from(&args.runtime_root),
        tick_interval_ms: args.runtime_tick_ms.max(1),
        local_worker_limit: if args.runtime_workers == 0 {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
                .max(1)
        } else {
            args.runtime_workers.max(1)
        },
        resume_existing_workspaces: env_bool(&[
            "NM_RUNTIME_RESUME_EXISTING_WORKSPACES",
            "NM_WEB_UI_RESUME_EXISTING_WORKSPACES",
            "AARNN_RUNTIME_RESUME_EXISTING_WORKSPACES",
        ])
        .unwrap_or(true),
        autosave_steps: args.runtime_autosave_steps.max(1),
        continuum: aarnn_rust::runtime::ContinuumAutoscalerConfig::from_env(),
        reconcile_interval_ms: env_opt("NM_RUNTIME_RECONCILE_INTERVAL_MS")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(1000),
        autoscaler_interval_ms: env_opt("NM_RUNTIME_AUTOSCALER_INTERVAL_MS")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(2000),
        orchestrator_addr: args.orchestrator.clone(),
    })
    .await?;

    let state = Arc::new(AppState {
        default_orchestrator: args.orchestrator,
        default_runtime_user: env_opt("NM_WEB_UI_DEFAULT_RUNTIME_USER"),
        auth,
        cors,
        users: Arc::new(RwLock::new(users)),
        session_store,
        runtime,
        chain,
        token_pricing,
        commerce,
    });

    let api = Router::new()
        .route("/openapi.json", get(openapi_json))
        .route("/config", get(api_config))
        .route("/auth/mode", get(auth_mode_handler))
        .route("/me", get(me))
        .route("/login", post(login))
        .route("/signup", post(signup))
        .route("/logout", post(logout))
        .route("/tokens", get(tokens_balance))
        .route("/tokens/ledger", get(tokens_ledger))
        .route("/user/config", get(get_user_config).post(set_user_config))
        .route("/runtime/status", get(runtime_status))
        .route(
            "/runtime/workspaces",
            get(runtime_workspaces).post(create_runtime_workspace),
        )
        .route(
            "/runtime/workspaces/{workspace_id}",
            get(runtime_workspace_detail).delete(delete_runtime_workspace),
        )
        .route(
            "/runtime/workspaces/{workspace_id}/snapshot",
            get(runtime_workspace_snapshot),
        )
        .route(
            "/runtime/workspaces/{workspace_id}/activity",
            get(runtime_workspace_activity),
        )
        .route(
            "/runtime/workspaces/{workspace_id}/control",
            post(control_runtime_workspace),
        )
        .route(
            "/runtime/workspaces/{workspace_id}/import",
            post(import_runtime_workspace),
        )
        .route(
            "/runtime/workspaces/{workspace_id}/export",
            get(export_runtime_workspace),
        )
        .route("/status", get(status))
        .route("/snapshot", get(snapshot))
        .route("/activity", get(activity))
        .route("/export", get(export))
        .route("/aer/inject", post(aer_inject))
        .route("/aer/stream", post(aer_stream))
        .route("/llm/mirror", post(llm_mirror))
        .route("/update_network", post(update_network))
        .route("/control_network", post(control_network))
        .layer(DefaultBodyLimit::max(grpc_max_message_bytes()))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            api_auth_middleware,
        ));

    let app = Router::new()
        .route("/", get(index))
        .route("/app.js", get(app_js))
        .route("/service-access.js", get(service_access_js))
        .route("/shell.js", get(shell_js))
        .route("/style.css", get(style_css))
        .route("/docs", get(docs_page))
        .route("/docs/", get(docs_page))
        .route("/docs/swagger", get(swagger_page))
        .route("/docs/swagger/", get(swagger_page))
        .route("/auth/access/exchange", post(access_exchange))
        .route("/auth/oidc/login", get(oidc_login))
        .route("/auth/oidc/callback", get(oidc_callback))
        .route("/auth/oidc/exchange", post(oidc_exchange))
        .nest("/api", api)
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            cors_middleware,
        ));

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
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, max-age=0"),
    );
    (headers, include_str!("../../web_ui/index.html"))
}

async fn app_js() -> impl IntoResponse {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("application/javascript; charset=utf-8"),
    );
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, max-age=0"),
    );
    (headers, include_str!("../../web_ui/app.js"))
}

async fn service_access_js() -> impl IntoResponse {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("application/javascript; charset=utf-8"),
    );
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, max-age=0"),
    );
    (headers, include_str!("../../web_ui/service-access.js"))
}

async fn shell_js() -> impl IntoResponse {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("application/javascript; charset=utf-8"),
    );
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, max-age=0"),
    );
    (headers, include_str!("../../web_ui/shell.js"))
}

async fn style_css() -> impl IntoResponse {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("text/css; charset=utf-8"),
    );
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, max-age=0"),
    );
    (headers, include_str!("../../web_ui/style.css"))
}

async fn docs_page() -> impl IntoResponse {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, max-age=0"),
    );
    (headers, include_str!("../../web_ui/docs.html"))
}

async fn swagger_page() -> impl IntoResponse {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, max-age=0"),
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
        "Current runtime auth mode: `{}`. Protected endpoints require either the `nm_session` cookie or a Customers-issued bearer token, plus the relevant `service_access.aarnn` authorisation. \
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
        { "name": "integration", "description": "Backend integration routes" },
        { "name": "oidc", "description": "OIDC browser and token exchange endpoints" }
      ],
      "components": {
        "securitySchemes": {
          "cookieAuth": {
            "type": "apiKey",
            "in": "cookie",
            "name": "nm_session",
            "description": "Session cookie set by /api/login or OIDC flow."
          },
          "bearerAuth": {
            "type": "http",
            "scheme": "bearer",
            "bearerFormat": "opaque",
            "description": "Customers-issued bearer token for backend integrations such as Gail."
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
          "ServiceAccessEntry": {
            "type": "object",
            "properties": {
              "service_key": { "type": "string" },
              "access_level": { "type": "string", "enum": ["none", "request", "observe", "use", "control"] },
              "public_access_level": { "type": "string", "enum": ["none", "request", "observe", "use", "control"] },
              "visible_access_level": { "type": "string", "enum": ["none", "request", "observe", "use", "control"] },
              "visible": { "type": "boolean" },
              "can_request": { "type": "boolean" },
              "can_observe": { "type": "boolean" },
              "can_use": { "type": "boolean" },
              "can_control": { "type": "boolean" }
            },
            "required": [
              "service_key",
              "access_level",
              "public_access_level",
              "visible_access_level",
              "visible",
              "can_request",
              "can_observe",
              "can_use",
              "can_control"
            ]
          },
          "AuthModeResponse": {
            "type": "object",
            "properties": {
              "mode": { "type": "string", "enum": ["none", "local", "oidc"] },
              "allow_signup": { "type": "boolean" },
              "central_auth": { "type": "boolean" }
            },
            "required": ["mode", "allow_signup", "central_auth"]
          },
          "UiConfigResponse": {
            "type": "object",
            "properties": {
              "default_orchestrator": { "type": "string", "nullable": true, "description": "Default gRPC orchestrator address." },
              "default_runtime_user": { "type": "string", "nullable": true, "description": "Default runtime workspace namespace used for anonymous browser sessions." },
              "shared_login_url": { "type": "string", "nullable": true, "description": "Commercial NeuralMimicry login URL used for cross-product SSO handoff." },
              "token_vault_url": { "type": "string", "nullable": true, "description": "Commercial token vault URL." },
              "buy_tokens_url": { "type": "string", "nullable": true, "description": "Deep link to the token vault buy-more flow." },
              "billing_dashboard_url": { "type": "string", "nullable": true, "description": "Commercial billing dashboard URL." },
              "billing_admin_url": { "type": "string", "nullable": true, "description": "Commercial admin billing dashboard URL." },
              "neuron_daily_rate": { "type": "integer", "format": "int64", "description": "Projected token burn per neuron per day." }
            }
          },
          "MeResponse": {
            "type": "object",
            "properties": {
              "authenticated": { "type": "boolean" },
              "mode": { "type": "string", "nullable": true },
              "user": { "type": "string", "nullable": true },
              "username": { "type": "string", "nullable": true },
              "role": { "type": "string", "nullable": true },
              "groups": { "type": "array", "items": { "type": "string" } },
              "email": { "type": "string", "nullable": true },
              "active_team": { "type": "object", "nullable": true, "additionalProperties": true },
              "team_count": { "type": "integer", "format": "int64" },
              "pending_invitation_count": { "type": "integer", "format": "int64" },
              "is_admin": { "type": "boolean" },
              "service_access": {
                "type": "object",
                "additionalProperties": { "$ref": "#/components/schemas/ServiceAccessEntry" }
              },
              "visible_services": { "type": "array", "items": { "type": "string" } }
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
              "authenticated": { "type": "boolean" },
              "user": { "type": "string", "nullable": true },
              "username": { "type": "string", "nullable": true },
              "role": { "type": "string", "nullable": true },
              "groups": { "type": "array", "items": { "type": "string" } },
              "email": { "type": "string", "nullable": true },
              "active_team": { "type": "object", "nullable": true, "additionalProperties": true },
              "team_count": { "type": "integer", "format": "int64" },
              "pending_invitation_count": { "type": "integer", "format": "int64" },
              "is_admin": { "type": "boolean" },
              "service_access": {
                "type": "object",
                "additionalProperties": { "$ref": "#/components/schemas/ServiceAccessEntry" }
              },
              "visible_services": { "type": "array", "items": { "type": "string" } },
              "access_token": { "type": "string", "nullable": true }
            },
            "required": ["ok", "authenticated", "username"]
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
              "telemetry_source": { "type": "string" },
              "telemetry_ts_ms": { "type": "integer", "format": "uint64" },
              "telemetry_cpu_usage_pct": { "type": "number" },
              "telemetry_mem_used_pct": { "type": "number" },
              "telemetry_net_rx_bps": { "type": "number" },
              "telemetry_net_tx_bps": { "type": "number" },
              "telemetry_disk_used_pct": { "type": "number" },
              "telemetry_disk_read_bps": { "type": "number" },
              "telemetry_disk_write_bps": { "type": "number" },
              "telemetry_gpu_util_pct": { "type": "number" },
              "telemetry_gpu_temp_c": { "type": "number" },
              "telemetry_gpu_power_w": { "type": "number" },
              "telemetry_gpu_mem_used_pct": { "type": "number" },
              "telemetry_recent_action_count": { "type": "integer", "format": "uint32" },
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
              "deployment_modes": { "type": "array", "items": { "type": "string" } },
              "deployment_scope": { "type": "string" },
              "live_transition_allowed": { "type": "boolean" },
              "autonomous_transition_enabled": { "type": "boolean" },
              "last_transition_reason": { "type": "string" },
              "last_transition_ts_ms": { "type": "integer", "format": "uint64" },
              "last_transition_source": { "type": "string" },
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
                  "time_ms": { "type": "number", "format": "float", "nullable": true, "description": "Optional physical time for temporal encoders such as phase coding." },
                  "dt_ms": { "type": "number", "format": "float", "nullable": true, "description": "Optional timestep used for temporal encoders when `time_ms` is omitted." },
                  "aer_base": { "type": "integer", "format": "uint32", "nullable": true, "description": "Base address for decoding AER payload." },
                  "aer_payload_hex": { "type": "string", "nullable": true, "description": "Hex-encoded AER1 payload bytes." },
                  "spike_indices": { "type": "array", "items": { "type": "integer", "format": "uint32" }, "nullable": true, "description": "Fallback direct sensory spike indices." },
                  "input_values": { "type": "array", "items": { "type": "number", "format": "float" }, "nullable": true, "description": "Continuous input values to encode into spikes using `spike_io`." },
                  "spike_io": { "type": "object", "nullable": true, "description": "Optional spike I/O override. Supports explicit profile/input/output selection including `ttfs`, `isi`, `phase`, and `multiplex`." },
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
          "LlmMirrorPayload": {
            "type": "object",
            "properties": {
              "request_id": { "type": "string" },
              "conversation_id": { "type": "string" },
              "workflow": { "type": "string" },
              "role": { "type": "string" },
              "direction": { "type": "string", "enum": ["input", "output"] },
              "provider": { "type": "string", "nullable": true },
              "model": { "type": "string", "nullable": true },
              "request_category": { "type": "string", "nullable": true },
              "system": { "type": "string", "nullable": true },
              "prompt_text": { "type": "string", "nullable": true },
              "text": { "type": "string" },
              "message_roles": { "type": "array", "items": { "type": "string" } },
              "aer_base": { "type": "integer", "format": "uint32" },
              "output_base": { "type": "integer", "format": "uint32" },
              "aer_payload_hex": { "type": "string" },
              "sensory_spikes": {
                "type": "array",
                "items": { "type": "integer", "format": "uint8" },
                "description": "Binary spike vector mirrored from Gail's SNN/AER translation layer."
              },
              "network_id": { "type": "string", "nullable": true },
              "node_id": { "type": "string", "nullable": true },
              "request_candidate_reply": { "type": "boolean" }
            },
            "required": [
              "request_id",
              "conversation_id",
              "workflow",
              "role",
              "direction",
              "text",
              "aer_base",
              "output_base",
              "aer_payload_hex"
            ]
          },
          "LlmMirrorCandidate": {
            "type": "object",
            "properties": {
              "reply_text": { "type": "string", "nullable": true },
              "confidence": { "type": "number", "format": "float", "nullable": true },
              "usable": { "type": "boolean" },
              "source": { "type": "string", "nullable": true },
              "output_spike_indices": {
                "type": "array",
                "items": { "type": "integer", "format": "uint32" }
              },
              "output_aer_payload_hex": { "type": "string", "nullable": true }
            },
            "required": ["usable", "output_spike_indices"]
          },
          "LlmMirrorStimulus": {
            "type": "object",
            "properties": {
              "attempted": { "type": "boolean" },
              "accepted_batches": { "type": "integer", "format": "uint64" },
              "target": { "type": "string", "nullable": true },
              "network_id": { "type": "string", "nullable": true },
              "error": { "type": "string", "nullable": true }
            },
            "required": ["attempted", "accepted_batches"]
          },
          "LlmMirrorResponse": {
            "type": "object",
            "properties": {
              "accepted": { "type": "boolean" },
              "request_id": { "type": "string" },
              "conversation_id": { "type": "string" },
              "direction": { "type": "string", "enum": ["input", "output"] },
              "text_chars": { "type": "integer", "format": "uint64" },
              "spike_count": { "type": "integer", "format": "uint64" },
              "aer_payload_hex": { "type": "string" },
              "candidate": {
                "$ref": "#/components/schemas/LlmMirrorCandidate",
                "nullable": true
              },
              "stimulation": { "$ref": "#/components/schemas/LlmMirrorStimulus" }
            },
            "required": [
              "accepted",
              "request_id",
              "conversation_id",
              "direction",
              "text_chars",
              "spike_count",
              "aer_payload_hex"
            ]
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
            "summary": "Sign-up (if enabled)",
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
              "200": { "description": "Sign-up successful.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/SignupResponse" } } } },
              "400": { "description": "Invalid request or local auth disabled.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "403": { "description": "Sign-up disabled.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
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
              "401": { "description": "Unauthorised.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } }
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
              "401": { "description": "Unauthorised.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } }
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
              "401": { "description": "Unauthorised.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "503": { "description": "Unable to connect to orchestrator.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } }
            }
          }
        },
        "/api/health": {
          "get": {
            "tags": ["health"],
            "summary": "Get system health",
            "operationId": "getSystemHealth",
            "security": [{ "cookieAuth": [] }],
            "parameters": [],
            "responses": {
              "200": { "description": "OK", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/StatusResponse" } } } },
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
              "401": { "description": "Unauthorised.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
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
              "401": { "description": "Unauthorised.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
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
              "400": { "description": "Missing orchestrator address or invalid update payload.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "403": { "description": "Cluster policy denied the requested live transition.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "404": { "description": "Target network was not found.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "401": { "description": "Unauthorised.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "409": { "description": "Live deployment transition requires explicit permission.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
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
              "401": { "description": "Unauthorised.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "500": { "description": "Update failed.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "503": { "description": "Connect failed.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } }
            }
          }
        },
            "/api/aer/inject": {
              "post": {
                "tags": ["network"],
                "summary": "Inject one AER exchange into a running network",
                "description": "Injects sensory spikes into the next simulation step. Accepts raw spike transports (`aer_payload_hex`, `spike_indices`) or continuous `input_values` that are encoded using the provided `spike_io` policy.",
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
                        },
                        "ttfsValues": {
                          "value": {
                            "network_id": "default",
                            "step_index": 3,
                            "input_values": [0.1, 0.6, 0.95],
                            "spike_io": {
                              "profile": "generic",
                              "input_strategy": "ttfs",
                              "ttfs": { "threshold": 0.0, "window_steps": 8 }
                            }
                          }
                        }
                      }
                    }
              }
            },
            "responses": {
              "200": { "description": "AER exchange accepted.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/AerInjectResponse" } } } },
              "400": { "description": "Invalid payload or missing fields.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "401": { "description": "Unauthorised.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "503": { "description": "Target connection/stream unavailable.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } }
            }
          }
        },
        "/api/aer/stream": {
              "post": {
                "tags": ["network"],
                "summary": "Inject a stream of AER exchanges (NDJSON over HTTP)",
                "description": "Accepts newline-delimited JSON frames in request body. Each frame can contain raw spike transport fields (`aer_payload_hex`, `spike_indices`) or `input_values` with a `spike_io` encoder selection. Use `Content-Type: application/x-ndjson`.",
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
                          "value": "{\"spike_indices\":[0,1,2]}\\n{\"input_values\":[0.2,0.9],\"spike_io\":{\"profile\":\"generic\",\"input_strategy\":\"phase\",\"phase\":{\"frequency_hz\":8.0,\"threshold\":0.55}}}\\n"
                        }
                      }
                    }
              }
            },
            "responses": {
              "200": { "description": "Stream accepted and forwarded.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/AerInjectResponse" } } } },
              "400": { "description": "Invalid NDJSON or missing data.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "401": { "description": "Unauthorised.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "503": { "description": "Target connection/stream unavailable.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } }
            }
          }
        },
        "/api/llm/mirror": {
          "post": {
            "tags": ["integration"],
            "summary": "Mirror Gail LLM I/O into the AARNN runtime",
            "description": "Accepts a mirrored Gail input or output exchange, persists the exchange beneath the runtime root, optionally stimulates the selected network with the supplied AER payload, and can return a low-confidence bootstrap candidate reply. This route is intended for backend integrations and is normally called with a Customers-issued Gail service-account bearer token.",
            "operationId": "mirrorLlmExchange",
            "security": [{ "cookieAuth": [] }, { "bearerAuth": [] }],
            "requestBody": {
              "required": true,
              "content": {
                "application/json": {
                  "schema": { "$ref": "#/components/schemas/LlmMirrorPayload" }
                }
              }
            },
            "responses": {
              "200": { "description": "Mirrored exchange accepted.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/LlmMirrorResponse" } } } },
              "400": { "description": "Invalid mirrored exchange payload.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
              "401": { "description": "Unauthorised.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } }
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
              "401": { "description": "Unauthorised.", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
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
              "303": { "description": "Redirect to OIDC authorisation endpoint." },
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
        let runtime_user = req
            .headers()
            .get("x-nm-runtime-user")
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("anonymous")
            .to_string();
        req.extensions_mut().insert(AuthUser::local(runtime_user));
        return next.run(req).await;
    }
    let path = req.uri().path();
    if is_public_api_path(path) {
        return next.run(req).await;
    }
    let access_requirement = api_access_requirement(req.method(), path);
    let jar = CookieJar::from_headers(req.headers());
    if let Some(user) = session_auth_user(&state, &jar).await {
        if let Some(requirement) = access_requirement {
            if !user.can_access(requirement) {
                return insufficient_service_access_response(&user, requirement);
            }
        }
        req.extensions_mut().insert(user);
        return next.run(req).await;
    }
    if let Some(user) = bearer_auth_user(&state, req.headers()).await {
        if let Some(requirement) = access_requirement {
            if !user.can_access(requirement) {
                return insufficient_service_access_response(&user, requirement);
            }
        }
        req.extensions_mut().insert(user);
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
        central_auth: state.auth.central.is_some(),
    })
}

async fn api_config(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Json(UiConfigResponse {
        default_orchestrator: state.default_orchestrator.clone(),
        default_runtime_user: state.default_runtime_user.clone(),
        shared_login_url: state.commerce.shared_login_url.clone(),
        token_vault_url: state.commerce.token_vault_url.clone(),
        buy_tokens_url: state.commerce.buy_tokens_url.clone(),
        billing_dashboard_url: state.commerce.billing_dashboard_url.clone(),
        billing_admin_url: state.commerce.billing_admin_url.clone(),
        neuron_daily_rate: state.token_pricing.neuron_daily_rate,
    })
}

fn forbid_shared_cluster_api(state: &AppState) -> Option<axum::response::Response> {
    if state.auth.mode == AuthMode::None {
        return None;
    }
    Some(
        (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "shared cluster-wide APIs are disabled for authenticated sessions; use /api/runtime/workspaces/*"
            })),
        )
            .into_response(),
    )
}

async fn me(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    jar: CookieJar,
) -> axum::response::Response {
    if state.auth.mode == AuthMode::None {
        return Json(json!({
            "authenticated": false,
            "mode": "none",
            "user": Value::Null,
            "username": Value::Null,
            "role": Value::Null,
            "groups": Vec::<String>::new(),
            "email": Value::Null,
            "active_team": Value::Null,
            "team_count": 0,
            "pending_invitation_count": 0,
            "is_admin": false,
            "service_access": json!({}),
            "visible_services": Vec::<String>::new(),
        }))
        .into_response();
    }
    if let Some(user) = session_auth_user(&state, &jar).await {
        return Json(authenticated_identity_payload(&user)).into_response();
    }
    if let Some(user) = bearer_auth_user(&state, &headers).await {
        return Json(authenticated_identity_payload(&user)).into_response();
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
    if let Some(central) = state.auth.central.as_ref() {
        let response = match central.login(username, password).await {
            Ok(response) => response,
            Err(err) => {
                let status = err.status.unwrap_or(502);
                let error = err
                    .payload
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("central_auth_failed");
                let details = err
                    .payload
                    .get("details")
                    .and_then(Value::as_str)
                    .unwrap_or(err.message.as_str());
                return (
                    StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY),
                    Json(json!({
                        "error": error,
                        "details": details,
                    })),
                )
                    .into_response();
            }
        };
        let access_token = match response
            .access_token
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(value) => Some(value.to_string()),
            None => {
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({ "error": "central_auth_invalid", "details": "central auth did not return an access token" })),
                )
                    .into_response();
            }
        };
        let Some(auth_user) = AuthUser::from_central_login(&response, username, access_token)
        else {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": "central_auth_invalid", "details": "central auth did not return a username" })),
            )
                .into_response();
        };
        let (jar, session_id) = match store_browser_session(&state, jar, &auth_user).await {
            Ok(result) => result,
            Err(err) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": format!("failed to persist session: {}", err) })),
                )
                    .into_response();
            }
        };
        chain_sync_identity_best_effort(&state, &auth_user, "local", None, "login").await;
        chain_record_login_best_effort(
            &state,
            &auth_user.username,
            "local",
            Some(session_id),
            "login",
        )
        .await;
        return (jar, Json(login_success_payload(&auth_user))).into_response();
    }

    let users = load_users_fresh(&state).await;
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

    let auth_user = AuthUser::local(username.to_string());
    let (jar, session_id) = match store_browser_session(&state, jar, &auth_user).await {
        Ok(result) => result,
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("failed to persist session: {}", err) })),
            )
                .into_response();
        }
    };
    chain_sync_identity_best_effort(&state, &auth_user, "local", None, "login").await;
    chain_record_login_best_effort(&state, username, "local", Some(session_id), "login").await;
    (jar, Json(login_success_payload(&auth_user))).into_response()
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
    if state.auth.central.is_some() {
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
    if let Err(err) = modify_users(&state, |users| {
        if users.find_by_username(username).is_some() {
            anyhow::bail!("user exists");
        }
        users.users.push(UserRecord {
            username: username.to_string(),
            password_hash: Some(hash.clone()),
            oidc_subject: None,
            oidc_issuer: None,
            email: None,
            created_at: now_ts(),
            config: None,
        });
        Ok(())
    })
    .await
    {
        let code = if err.to_string() == "user exists" {
            StatusCode::CONFLICT
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        };
        return (code, Json(json!({ "error": err.to_string() }))).into_response();
    }
    let auth_user = AuthUser::local(username.to_string());
    chain_sync_identity_best_effort(&state, &auth_user, "local", None, "signup").await;
    Json(json!({ "ok": true })).into_response()
}

async fn logout(State(state): State<Arc<AppState>>, jar: CookieJar) -> impl IntoResponse {
    if let Some(cookie) = jar.get("nm_session") {
        let session_id = cookie.value().to_string();
        let _ = state.session_store.delete(&session_id).await;
    }
    let mut expired = Cookie::new("nm_session", "");
    expired.set_path("/");
    let jar = jar.remove(expired);
    (jar, Json(json!({ "ok": true })))
}

async fn access_exchange(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Form(payload): Form<AccessExchangePayload>,
) -> impl IntoResponse {
    let next_path = safe_next_path(payload.next);
    let Some(central) = state.auth.central.as_ref() else {
        return Redirect::to(&next_path).into_response();
    };
    let Some(access_token) = payload
        .access_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Redirect::to(&next_path).into_response();
    };
    let session = match central.session(access_token).await {
        Ok(session) => session,
        Err(_) => return Redirect::to(&next_path).into_response(),
    };
    if !session.authenticated {
        return Redirect::to(&next_path).into_response();
    }
    let Some(auth_user) = AuthUser::from_central_session(&session, Some(access_token.to_string()))
    else {
        return Redirect::to(&next_path).into_response();
    };
    let (jar, session_id) = match store_browser_session(&state, jar, &auth_user).await {
        Ok(result) => result,
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("failed to persist session: {}", err) })),
            )
                .into_response();
        }
    };
    chain_sync_identity_best_effort(
        &state,
        &auth_user,
        "central_access",
        None,
        "access_exchange",
    )
    .await;
    chain_record_login_best_effort(
        &state,
        &auth_user.username,
        "central_access",
        Some(session_id),
        "access_exchange",
    )
    .await;
    (jar, Redirect::to(&next_path)).into_response()
}

fn pricing_payload(pricing: &TokenPricing) -> Value {
    json!({
        "create_workspace": pricing.create_workspace,
        "import_workspace": pricing.import_workspace,
        "start_workspace": pricing.start_workspace,
        "repeat_workspace": pricing.repeat_workspace,
        "step_workspace": pricing.step_workspace,
    })
}

async fn chain_sync_identity_best_effort(
    state: &AppState,
    user: &AuthUser,
    provider: &str,
    subject: Option<String>,
    source: &str,
) {
    let Some(chain) = state.chain.as_ref() else {
        return;
    };
    let payload = NmChainIdentityUpsertRequest {
        request_id: None,
        user_id: user.username.clone(),
        role: Some(user.role.clone()),
        email: user.email.clone(),
        provider: Some(provider.to_string()),
        subject,
        meta: json!({
            "source": source,
            "groups": user.groups.clone(),
            "active_team": user.active_team.clone(),
            "team_count": user.team_count,
            "pending_invitation_count": user.pending_invitation_count,
            "is_admin": user.is_admin,
            "service_access": user.service_access.clone(),
            "visible_services": user.visible_services(),
        }),
    };
    if let Err(err) = chain.upsert_identity(&payload).await {
        eprintln!(
            "[warn] nmchain identity upsert failed for {}: {}",
            user.username, err
        );
    }
}

async fn chain_record_login_best_effort(
    state: &AppState,
    username: &str,
    auth_mode: &str,
    session_id: Option<String>,
    source: &str,
) {
    let Some(chain) = state.chain.as_ref() else {
        return;
    };
    let payload = NmChainLoginObservedRequest {
        request_id: None,
        user_id: username.to_string(),
        system: chain.app_id().to_string(),
        auth_mode: Some(auth_mode.to_string()),
        session_id,
        remote_addr: None,
        meta: json!({ "source": source }),
    };
    if let Err(err) = chain.observe_login(&payload).await {
        eprintln!(
            "[warn] nmchain login record failed for {}: {}",
            username, err
        );
    }
}

async fn chain_token_snapshot(
    state: &AppState,
    username: &str,
) -> anyhow::Result<Option<NmChainAccountSnapshot>> {
    let Some(chain) = state.chain.as_ref() else {
        return Ok(None);
    };
    chain
        .account_snapshot("user", username)
        .await
        .map(Some)
        .context("failed to fetch nmchain token snapshot")
}

async fn chain_token_entries(
    state: &AppState,
    username: &str,
    limit: usize,
) -> anyhow::Result<Vec<aarnn_rust::nmchain::NmChainLedgerEntry>> {
    let Some(chain) = state.chain.as_ref() else {
        return Ok(Vec::new());
    };
    let response: NmChainLedgerResponse = chain
        .ledger_entries("user", username, limit)
        .await
        .context("failed to fetch nmchain token ledger")?;
    Ok(response.entries)
}

async fn shared_token_snapshot(
    state: &AppState,
    access_token: &str,
) -> Result<Option<CentralTokenSnapshot>, CentralApiError> {
    let Some(billing) = state.auth.billing.as_ref() else {
        return Ok(None);
    };
    billing.token_snapshot(access_token).await.map(Some)
}

async fn shared_token_entries(
    state: &AppState,
    access_token: &str,
    limit: usize,
) -> Result<Vec<aarnn_rust::central_auth::CentralTokenLedgerEntry>, CentralApiError> {
    let Some(billing) = state.auth.billing.as_ref() else {
        return Ok(Vec::new());
    };
    let response: CentralTokenLedgerResponse = billing.token_ledger(access_token, limit).await?;
    Ok(response.entries)
}

async fn ensure_token_budget(state: &AppState, user: &AuthUser, cost: i64) -> anyhow::Result<()> {
    if cost <= 0 {
        return Ok(());
    }
    if let Some(access_token) = user.access_token.as_deref() {
        if let Some(snapshot) = shared_token_snapshot(state, access_token)
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?
        {
            if snapshot.available < cost {
                anyhow::bail!(
                    "insufficient tokens: need {}, available {}",
                    cost,
                    snapshot.available
                );
            }
            return Ok(());
        }
    }
    let Some(snapshot) = chain_token_snapshot(state, &user.username).await? else {
        return Ok(());
    };
    if snapshot.available < cost {
        anyhow::bail!(
            "insufficient tokens: need {}, available {}",
            cost,
            snapshot.available
        );
    }
    Ok(())
}

async fn debit_tokens(
    state: &AppState,
    user: &AuthUser,
    amount: i64,
    request_id: String,
    meta: Value,
) -> anyhow::Result<()> {
    if amount <= 0 {
        return Ok(());
    }
    if let Some(access_token) = user.access_token.as_deref() {
        if let Some(billing) = state.auth.billing.as_ref() {
            let response: CentralTokenActionResponse = billing
                .debit_tokens(access_token, amount, &request_id, meta)
                .await
                .map_err(|err| anyhow::anyhow!(err.to_string()))?;
            let used_total = response.used.unwrap_or_else(|| {
                response
                    .entry
                    .as_ref()
                    .map(|entry| entry.delta.abs())
                    .unwrap_or(amount)
            });
            let shortfall = response.shortfall.unwrap_or_else(|| {
                response
                    .entry
                    .as_ref()
                    .map(|entry| entry.shortfall)
                    .unwrap_or(0)
            });
            if shortfall > 0 || used_total < amount {
                anyhow::bail!(
                    "token debit shortfall: {}",
                    shortfall.max(amount - used_total)
                );
            }
            return Ok(());
        }
    }
    let Some(chain) = state.chain.as_ref() else {
        return Ok(());
    };
    let result = chain
        .apply_token(&NmChainTokenMutationRequest {
            request_id: Some(request_id),
            account_scope: "user".to_string(),
            account_id: user.username.to_string(),
            entry_type: "debit".to_string(),
            delta: -amount,
            meta,
        })
        .await
        .context("failed to debit tokens from nmchain")?;
    if let Some(entry) = result.entry {
        if entry.shortfall > 0 {
            anyhow::bail!("token debit shortfall: {}", entry.shortfall);
        }
    }
    Ok(())
}

async fn refund_tokens(
    state: &AppState,
    user: &AuthUser,
    amount: i64,
    request_id: String,
    meta: Value,
) {
    if amount <= 0 {
        return;
    }
    if let Some(access_token) = user.access_token.as_deref() {
        if let Some(billing) = state.auth.billing.as_ref() {
            if let Err(err) = billing
                .refund_tokens(access_token, amount, &request_id, meta)
                .await
            {
                eprintln!(
                    "[warn] billing refund failed for {}: {}",
                    user.username, err
                );
            }
            return;
        }
    }
    let Some(chain) = state.chain.as_ref() else {
        return;
    };
    let payload = NmChainTokenMutationRequest {
        request_id: Some(request_id),
        account_scope: "user".to_string(),
        account_id: user.username.to_string(),
        entry_type: "refund".to_string(),
        delta: amount,
        meta,
    };
    if let Err(err) = chain.apply_token(&payload).await {
        eprintln!(
            "[warn] nmchain refund failed for {}: {}",
            user.username, err
        );
    }
}

async fn tokens_balance(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
) -> axum::response::Response {
    let pricing = pricing_payload(&state.token_pricing);
    let neuron_daily_rate = state.token_pricing.neuron_daily_rate;
    let token_vault_url = state.commerce.token_vault_url.clone();
    let buy_tokens_url = state.commerce.buy_tokens_url.clone();
    let billing_dashboard_url = state.commerce.billing_dashboard_url.clone();
    let billing_admin_url = state.commerce.billing_admin_url.clone();
    if let Some(access_token) = user.access_token.as_deref() {
        if let Some(billing) = state.auth.billing.as_ref() {
            return match billing.token_snapshot(access_token).await {
                Ok(snapshot) => Json(json!({
                    "configured": true,
                    "pricing": pricing,
                    "balance": snapshot.balance,
                    "tokens": snapshot.tokens,
                    "paid_balance": snapshot.paid_balance,
                    "free_balance": snapshot.free_balance,
                    "available": snapshot.available,
                    "reserved": snapshot.reserved,
                    "in_use": snapshot.in_use,
                    "capacity": snapshot.capacity,
                    "display_capacity": snapshot.display_capacity,
                    "low_threshold": snapshot.low_threshold,
                    "status": snapshot.status,
                    "last_topup_tokens": snapshot.last_topup_tokens,
                    "last_topup_at": snapshot.last_topup_at,
                    "updated_at": snapshot.updated_at,
                    "spent_total": snapshot.spent_total,
                    "cashout_total": snapshot.cashout_total,
                    "shortfall_total": snapshot.shortfall_total,
                    "free_grant_total": snapshot.free_grant_total,
                    "identity": snapshot.identity,
                    "neuron_daily_rate": neuron_daily_rate,
                    "token_vault_url": token_vault_url,
                    "buy_tokens_url": buy_tokens_url,
                    "billing_dashboard_url": billing_dashboard_url,
                    "billing_admin_url": billing_admin_url,
                }))
                .into_response(),
                Err(err) => (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({ "error": err.to_string() })),
                )
                    .into_response(),
            };
        }
    }
    match chain_token_snapshot(&state, &user.username).await {
        Ok(Some(snapshot)) => Json(json!({
            "configured": true,
            "pricing": pricing,
            "balance": snapshot.balance,
            "tokens": snapshot.tokens,
            "paid_balance": snapshot.paid_balance,
            "free_balance": snapshot.free_balance,
            "available": snapshot.available,
            "reserved": snapshot.reserved,
            "in_use": snapshot.in_use,
            "capacity": snapshot.capacity,
            "display_capacity": snapshot.display_capacity,
            "low_threshold": snapshot.low_threshold,
            "status": snapshot.status,
            "last_topup_tokens": snapshot.last_topup_tokens,
            "last_topup_at": snapshot.last_topup_at,
            "updated_at": snapshot.updated_at,
            "spent_total": snapshot.spent_total,
            "cashout_total": snapshot.cashout_total,
            "shortfall_total": snapshot.shortfall_total,
            "free_grant_total": snapshot.free_grant_total,
            "identity": snapshot.identity,
            "neuron_daily_rate": neuron_daily_rate,
            "token_vault_url": token_vault_url,
            "buy_tokens_url": buy_tokens_url,
            "billing_dashboard_url": billing_dashboard_url,
            "billing_admin_url": billing_admin_url,
        }))
        .into_response(),
        Ok(None) => Json(json!({
            "configured": false,
            "pricing": pricing,
            "balance": 0,
            "tokens": 0,
            "paid_balance": 0,
            "free_balance": 0,
            "available": 0,
            "reserved": 0,
            "in_use": 0,
            "capacity": 0,
            "display_capacity": 1,
            "low_threshold": 0,
            "status": "unconfigured",
            "last_topup_tokens": 0,
            "last_topup_at": null,
            "updated_at": null,
            "spent_total": 0,
            "cashout_total": 0,
            "shortfall_total": 0,
            "free_grant_total": 0,
            "identity": null,
            "neuron_daily_rate": neuron_daily_rate,
            "token_vault_url": token_vault_url,
            "buy_tokens_url": buy_tokens_url,
            "billing_dashboard_url": billing_dashboard_url,
            "billing_admin_url": billing_admin_url,
        }))
        .into_response(),
        Err(err) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn tokens_ledger(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Query(query): Query<TokenLedgerQuery>,
) -> axum::response::Response {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    if let Some(access_token) = user.access_token.as_deref() {
        if state.auth.billing.is_some() {
            return match shared_token_entries(&state, access_token, limit).await {
                Ok(entries) => Json(json!({
                    "configured": true,
                    "entries": entries,
                }))
                .into_response(),
                Err(err) => (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({ "error": err.to_string() })),
                )
                    .into_response(),
            };
        }
    }
    match chain_token_entries(&state, &user.username, limit).await {
        Ok(entries) => Json(json!({
            "configured": state.chain.is_some(),
            "entries": entries,
        }))
        .into_response(),
        Err(err) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn get_user_config(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
) -> axum::response::Response {
    if state.auth.mode == AuthMode::None {
        return Json(json!({ "config": {} })).into_response();
    }
    let users = load_users_fresh(&state).await;
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
    if let Err(err) = modify_users(&state, |users| {
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
        rec.config = Some(payload.config.clone());
        Ok(())
    })
    .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": err.to_string() })),
        )
            .into_response();
    }
    Json(json!({ "ok": true })).into_response()
}

async fn runtime_status(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
) -> axum::response::Response {
    match state
        .runtime
        .runtime_status_for_users(&user.username, runtime_workspace_scope_owners(&user))
        .await
    {
        Ok(status) => Json(status).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn runtime_workspaces(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
) -> axum::response::Response {
    match state
        .runtime
        .list_workspaces_for_users(runtime_workspace_scope_owners(&user))
        .await
    {
        Ok(workspaces) => Json(workspaces).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn create_runtime_workspace(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Json(payload): Json<WorkspaceCreateRequest>,
) -> axum::response::Response {
    let cost = state.token_pricing.create_workspace;
    if let Err(err) = ensure_token_budget(&state, &user, cost).await {
        return (
            StatusCode::PAYMENT_REQUIRED,
            Json(json!({ "error": err.to_string(), "required_tokens": cost })),
        )
            .into_response();
    }
    match state
        .runtime
        .create_workspace(&user.username, payload)
        .await
    {
        Ok(detail) => {
            if cost > 0 {
                let workspace_id = detail.summary.workspace_id.clone();
                let request_id = format!(
                    "aarnn:create:{}:{}",
                    user.username.trim(),
                    workspace_id.trim()
                );
                let debit = debit_tokens(
                    &state,
                    &user,
                    cost,
                    request_id,
                    json!({
                        "operation": "create_workspace",
                        "workspace_id": workspace_id,
                        "network_id": detail.summary.network_id,
                        "source": "aarnn_web_ui",
                    }),
                )
                .await;
                if let Err(err) = debit {
                    let _ = state
                        .runtime
                        .delete_workspace(&user.username, &detail.summary.workspace_id)
                        .await;
                    return (
                        StatusCode::PAYMENT_REQUIRED,
                        Json(json!({ "error": err.to_string(), "required_tokens": cost })),
                    )
                        .into_response();
                }
            }
            Json(detail).into_response()
        }
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn runtime_workspace_detail(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(workspace_id): Path<String>,
    Query(query): Query<RuntimeWorkspaceOwnerQuery>,
) -> axum::response::Response {
    let owner = match resolve_runtime_workspace_owner(&user, query.owner.as_deref()) {
        Ok(owner) => owner,
        Err(response) => return response,
    };
    match state.runtime.workspace_detail(&owner, &workspace_id).await {
        Ok(detail) => Json(detail).into_response(),
        Err(err) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn delete_runtime_workspace(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(workspace_id): Path<String>,
    Query(query): Query<RuntimeWorkspaceOwnerQuery>,
) -> axum::response::Response {
    let owner = match resolve_runtime_workspace_owner(&user, query.owner.as_deref()) {
        Ok(owner) => owner,
        Err(response) => return response,
    };
    match state.runtime.delete_workspace(&owner, &workspace_id).await {
        Ok(resp) => Json(resp).into_response(),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn runtime_workspace_snapshot(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(workspace_id): Path<String>,
    Query(query): Query<RuntimeWorkspaceSnapshotQuery>,
) -> axum::response::Response {
    let owner = match resolve_runtime_workspace_owner(&user, query.owner.as_deref()) {
        Ok(owner) => owner,
        Err(response) => return response,
    };
    match state
        .runtime
        .workspace_saved_snapshot(&owner, &workspace_id, query.if_saved_after_ms)
        .await
    {
        Ok(Some(snapshot)) => Json(snapshot).into_response(),
        Ok(None) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn runtime_workspace_activity(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(workspace_id): Path<String>,
    Query(query): Query<RuntimeWorkspaceOwnerQuery>,
) -> axum::response::Response {
    let owner = match resolve_runtime_workspace_owner(&user, query.owner.as_deref()) {
        Ok(owner) => owner,
        Err(response) => return response,
    };
    match state
        .runtime
        .workspace_activity(&owner, &workspace_id)
        .await
    {
        Ok(activity) => Json(activity).into_response(),
        Err(err) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn control_runtime_workspace(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(workspace_id): Path<String>,
    Query(query): Query<RuntimeWorkspaceOwnerQuery>,
    Json(payload): Json<WorkspaceControlRequest>,
) -> axum::response::Response {
    let requirement = workspace_control_requirement(payload.action);
    if !user.can_access(requirement) {
        return insufficient_service_access_response(&user, requirement);
    }
    let owner = match resolve_runtime_workspace_owner(&user, query.owner.as_deref()) {
        Ok(owner) => owner,
        Err(response) => return response,
    };
    let cost = state.token_pricing.control_cost(payload.action);
    if let Err(err) = ensure_token_budget(&state, &user, cost).await {
        return (
            StatusCode::PAYMENT_REQUIRED,
            Json(json!({ "error": err.to_string(), "required_tokens": cost })),
        )
            .into_response();
    }
    let action_name = control_action_label(payload.action);
    let debit_request_id = format!(
        "aarnn:control:{}:{}:{}:{}:{}",
        user.username.trim(),
        owner.trim(),
        workspace_id.trim(),
        action_name,
        now_ts()
    );
    if let Err(err) = debit_tokens(
        &state,
        &user,
        cost,
        debit_request_id.clone(),
        json!({
            "operation": "control_workspace",
            "workspace_owner": owner,
            "workspace_id": workspace_id,
            "action": action_name,
            "source": "aarnn_web_ui",
        }),
    )
    .await
    {
        return (
            StatusCode::PAYMENT_REQUIRED,
            Json(json!({ "error": err.to_string(), "required_tokens": cost })),
        )
            .into_response();
    }
    match state
        .runtime
        .control_workspace(&owner, &workspace_id, payload.action)
        .await
    {
        Ok(detail) => Json(detail).into_response(),
        Err(err) => {
            refund_tokens(
                &state,
                &user,
                cost,
                format!("refund:{}", debit_request_id),
                json!({
                    "operation": "control_workspace_refund",
                    "workspace_owner": owner,
                    "workspace_id": workspace_id,
                    "action": action_name,
                    "source": "aarnn_web_ui",
                }),
            )
            .await;
            (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": err.to_string() })),
            )
                .into_response()
        }
    }
}

async fn import_runtime_workspace(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(workspace_id): Path<String>,
    Query(query): Query<RuntimeWorkspaceOwnerQuery>,
    Json(payload): Json<WorkspaceImportRequest>,
) -> axum::response::Response {
    let owner = match resolve_runtime_workspace_owner(&user, query.owner.as_deref()) {
        Ok(owner) => owner,
        Err(response) => return response,
    };
    let cost = state.token_pricing.import_workspace;
    if let Err(err) = ensure_token_budget(&state, &user, cost).await {
        return (
            StatusCode::PAYMENT_REQUIRED,
            Json(json!({ "error": err.to_string(), "required_tokens": cost })),
        )
            .into_response();
    }
    match state
        .runtime
        .import_workspace_json(&owner, &workspace_id, payload)
        .await
    {
        Ok(detail) => {
            if cost > 0 {
                let request_id = format!(
                    "aarnn:import:{}:{}:{}:{}",
                    user.username.trim(),
                    owner.trim(),
                    workspace_id.trim(),
                    now_ts()
                );
                if let Err(err) = debit_tokens(
                    &state,
                    &user,
                    cost,
                    request_id,
                    json!({
                        "operation": "import_workspace",
                        "workspace_owner": owner,
                        "workspace_id": workspace_id,
                        "network_id": detail.summary.network_id,
                        "source": "aarnn_web_ui",
                    }),
                )
                .await
                {
                    return (
                        StatusCode::PAYMENT_REQUIRED,
                        Json(json!({ "error": err.to_string(), "required_tokens": cost })),
                    )
                        .into_response();
                }
            }
            Json(detail).into_response()
        }
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
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
    if let Err(err) = oidc
        .pending
        .put(
            &state_key,
            &OidcPendingRecord {
                nonce: nonce.secret().to_string(),
                pkce_verifier: pkce_verifier.secret().to_string(),
            },
        )
        .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("failed to persist oidc state: {}", err) })),
        )
            .into_response();
    }
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
    let pending = oidc.pending.take(&query.state).await;
    let pending = match pending {
        Ok(pending) => pending,
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("failed to load oidc state: {}", err) })),
            )
                .into_response();
        }
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
    let pending_nonce = Nonce::new(pending.nonce);
    let pending_pkce_verifier = PkceCodeVerifier::new(pending.pkce_verifier);

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
        .set_pkce_verifier(pending_pkce_verifier)
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
    let claims = match id_token.claims(&oidc.client.id_token_verifier(), &pending_nonce) {
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
    let raw_id_token = id_token.to_string();
    let raw_access_token = token_resp.access_token().secret().to_string();

    if let Some(central) = state.auth.central.as_ref() {
        let response = match central
            .oidc_exchange(&raw_id_token, Some(raw_access_token.as_str()))
            .await
        {
            Ok(response) => response,
            Err(err) => {
                return (
                    StatusCode::from_u16(err.status.unwrap_or(502))
                        .unwrap_or(StatusCode::BAD_GATEWAY),
                    Json(json!({
                        "error": err
                            .payload
                            .get("error")
                            .and_then(Value::as_str)
                            .unwrap_or("central_auth_failed"),
                        "details": err
                            .payload
                            .get("details")
                            .and_then(Value::as_str)
                            .unwrap_or(err.message.as_str()),
                    })),
                )
                    .into_response();
            }
        };
        let fallback_username = email
            .as_deref()
            .and_then(|value| value.split('@').next())
            .unwrap_or("oidc");
        let access_token = match response
            .access_token
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(value) => Some(value.to_string()),
            None => {
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({ "error": "central_auth_invalid", "details": "central auth did not return an access token" })),
                )
                    .into_response();
            }
        };
        let Some(auth_user) =
            AuthUser::from_central_login(&response, fallback_username, access_token)
        else {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": "central_auth_invalid", "details": "central auth did not return a username" })),
            )
                .into_response();
        };
        let (jar, session_id) = match store_browser_session(&state, jar, &auth_user).await {
            Ok(result) => result,
            Err(err) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": format!("failed to persist session: {}", err) })),
                )
                    .into_response();
            }
        };
        chain_sync_identity_best_effort(
            &state,
            &auth_user,
            "oidc",
            Some(subject.clone()),
            "oidc_callback",
        )
        .await;
        chain_record_login_best_effort(
            &state,
            &auth_user.username,
            "oidc",
            Some(session_id),
            "oidc_callback",
        )
        .await;
        return (jar, Redirect::to("/")).into_response();
    }

    let username = match modify_users(&state, |users| {
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
        Ok(username)
    })
    .await
    {
        Ok(username) => username,
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("failed to persist user: {}", err) })),
            )
                .into_response();
        }
    };

    let auth_user = AuthUser::local_with_email(username.clone(), email.clone());
    let (jar, session_id) = match store_browser_session(&state, jar, &auth_user).await {
        Ok(result) => result,
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("failed to persist session: {}", err) })),
            )
                .into_response();
        }
    };
    chain_sync_identity_best_effort(
        &state,
        &auth_user,
        "oidc",
        Some(subject.clone()),
        "oidc_callback",
    )
    .await;
    chain_record_login_best_effort(
        &state,
        &auth_user.username,
        "oidc",
        Some(session_id),
        "oidc_callback",
    )
    .await;
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

    if let Some(central) = state.auth.central.as_ref() {
        let raw_id_token = match payload
            .id_token
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(value) => value,
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "id_token required" })),
                )
                    .into_response();
            }
        };
        let response = match central
            .oidc_exchange(
                raw_id_token,
                payload
                    .access_token
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty()),
            )
            .await
        {
            Ok(response) => response,
            Err(err) => {
                return (
                    StatusCode::from_u16(err.status.unwrap_or(502))
                        .unwrap_or(StatusCode::BAD_GATEWAY),
                    Json(json!({
                        "error": err
                            .payload
                            .get("error")
                            .and_then(Value::as_str)
                            .unwrap_or("central_auth_failed"),
                        "details": err
                            .payload
                            .get("details")
                            .and_then(Value::as_str)
                            .unwrap_or(err.message.as_str()),
                    })),
                )
                    .into_response();
            }
        };
        let fallback_username = preferred_username
            .clone()
            .or_else(|| {
                email
                    .as_ref()
                    .and_then(|value| value.split('@').next().map(|part| part.to_string()))
            })
            .unwrap_or_else(|| "oidc".to_string());
        let access_token = match response
            .access_token
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(value) => Some(value.to_string()),
            None => {
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({ "error": "central_auth_invalid", "details": "central auth did not return an access token" })),
                )
                    .into_response();
            }
        };
        let Some(auth_user) =
            AuthUser::from_central_login(&response, &fallback_username, access_token)
        else {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": "central_auth_invalid", "details": "central auth did not return a username" })),
            )
                .into_response();
        };
        let (jar, session_id) = match store_browser_session(&state, jar, &auth_user).await {
            Ok(result) => result,
            Err(err) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": format!("failed to persist session: {}", err) })),
                )
                    .into_response();
            }
        };
        chain_sync_identity_best_effort(
            &state,
            &auth_user,
            "oidc",
            Some(subject.clone()),
            "oidc_exchange",
        )
        .await;
        chain_record_login_best_effort(
            &state,
            &auth_user.username,
            "oidc",
            Some(session_id),
            "oidc_exchange",
        )
        .await;
        return (jar, Redirect::to(&next_path)).into_response();
    }

    let username = match modify_users(&state, |users| {
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
        Ok(username)
    })
    .await
    {
        Ok(username) => username,
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("failed to persist user: {}", err) })),
            )
                .into_response();
        }
    };

    let auth_user = AuthUser::local_with_email(username.clone(), email.clone());
    let (jar, session_id) = match store_browser_session(&state, jar, &auth_user).await {
        Ok(result) => result,
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("failed to persist session: {}", err) })),
            )
                .into_response();
        }
    };
    chain_sync_identity_best_effort(
        &state,
        &auth_user,
        "oidc",
        Some(subject.clone()),
        "oidc_exchange",
    )
    .await;
    chain_record_login_best_effort(
        &state,
        &auth_user.username,
        "oidc",
        Some(session_id),
        "oidc_exchange",
    )
    .await;
    (jar, Redirect::to(&next_path)).into_response()
}

async fn session_auth_user(state: &AppState, jar: &CookieJar) -> Option<AuthUser> {
    let session_id = jar.get("nm_session")?.value().to_string();
    let session = match state.session_store.get(&session_id).await {
        Ok(session) => session,
        Err(_) => return None,
    }?;
    if session.expires_at > now_ts() {
        return Some(AuthUser::from_session_record(session));
    }
    let _ = state.session_store.delete(&session_id).await;
    None
}

async fn bearer_auth_user(state: &AppState, headers: &HeaderMap) -> Option<AuthUser> {
    let central = state.auth.central.as_ref()?;
    let access_token = extract_bearer_token(headers)?;
    let session = central.session(&access_token).await.ok()?;
    if !session.authenticated {
        return None;
    }
    AuthUser::from_central_session(&session, Some(access_token))
}

async fn status(
    State(state): State<Arc<AppState>>,
    Query(query): Query<StatusQuery>,
) -> impl IntoResponse {
    if let Some(resp) = forbid_shared_cluster_api(state.as_ref()) {
        return resp;
    }
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
                .into_response();
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
                "telemetry_source": res.map(|r| r.telemetry_source.clone()).unwrap_or_default(),
                "telemetry_ts_ms": res.map(|r| r.telemetry_ts_ms).unwrap_or(0),
                "telemetry_cpu_usage_pct": res.map(|r| r.telemetry_cpu_usage_pct).unwrap_or(0.0),
                "telemetry_mem_used_pct": res.map(|r| r.telemetry_mem_used_pct).unwrap_or(0.0),
                "telemetry_net_rx_bps": res.map(|r| r.telemetry_net_rx_bps).unwrap_or(0.0),
                "telemetry_net_tx_bps": res.map(|r| r.telemetry_net_tx_bps).unwrap_or(0.0),
                "telemetry_disk_used_pct": res.map(|r| r.telemetry_disk_used_pct).unwrap_or(0.0),
                "telemetry_disk_read_bps": res.map(|r| r.telemetry_disk_read_bps).unwrap_or(0.0),
                "telemetry_disk_write_bps": res.map(|r| r.telemetry_disk_write_bps).unwrap_or(0.0),
                "telemetry_gpu_util_pct": res.map(|r| r.telemetry_gpu_util_pct).unwrap_or(0.0),
                "telemetry_gpu_temp_c": res.map(|r| r.telemetry_gpu_temp_c).unwrap_or(0.0),
                "telemetry_gpu_power_w": res.map(|r| r.telemetry_gpu_power_w).unwrap_or(0.0),
                "telemetry_gpu_mem_used_pct": res.map(|r| r.telemetry_gpu_mem_used_pct).unwrap_or(0.0),
                "telemetry_recent_action_count": res.map(|r| r.telemetry_recent_action_count).unwrap_or(0),
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
                "deployment_modes": n.deployment_modes,
                "deployment_scope": n.deployment_scope,
                "live_transition_allowed": n.live_transition_allowed,
                "autonomous_transition_enabled": n.autonomous_transition_enabled,
                "last_transition_reason": n.last_transition_reason,
                "last_transition_ts_ms": n.last_transition_ts_ms,
                "last_transition_source": n.last_transition_source,
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
    if let Some(resp) = forbid_shared_cluster_api(state.as_ref()) {
        return resp;
    }
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
                .into_response();
        }
    };

    let target_addrs =
        match resolve_network_addrs(addr.clone(), &network_id, query.node_id.clone()).await {
            Ok(addrs) => addrs,
            Err(resp) => return resp.into_response(),
        };

    let mut last_error = String::from("no candidate target attempted");
    for target_addr in target_addrs {
        let mut client = match connect_cluster_client(target_addr.clone()).await {
            Ok(client) => client,
            Err(e) => {
                last_error = format!("connect failed via {}: {}", target_addr, e);
                continue;
            }
        };

        match client
            .get_network_snapshot(Request::new(NetworkSnapshotRequest {
                network_id: network_id.clone(),
            }))
            .await
        {
            Ok(resp) => {
                let resp = resp.into_inner();
                return (
                    StatusCode::OK,
                    Json(json!({
                        "network_id": resp.network_id,
                        "snapshot_json": resp.snapshot_json,
                        "source": target_addr,
                    })),
                )
                    .into_response();
            }
            Err(e) => {
                last_error = format!("snapshot failed via {}: {}", target_addr, e);
            }
        }
    }

    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({ "error": last_error })),
    )
        .into_response()
}

async fn activity(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ActivityQuery>,
) -> impl IntoResponse {
    if let Some(resp) = forbid_shared_cluster_api(state.as_ref()) {
        return resp;
    }
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
                .into_response();
        }
    };

    let target_addrs =
        match resolve_network_addrs(addr.clone(), &network_id, query.node_id.clone()).await {
            Ok(addrs) => addrs,
            Err(resp) => return resp.into_response(),
        };

    let mut last_error = String::from("no candidate target attempted");
    for target_addr in target_addrs {
        let mut client = match connect_cluster_client(target_addr.clone()).await {
            Ok(client) => client,
            Err(e) => {
                last_error = format!("connect failed via {}: {}", target_addr, e);
                continue;
            }
        };

        match client
            .get_network_activity(Request::new(NetworkActivityRequest {
                network_id: network_id.clone(),
            }))
            .await
        {
            Ok(resp) => {
                let resp = resp.into_inner();
                let sensory = resp.sensory.map(|s| s.indices).unwrap_or_default();
                let hidden: Vec<Vec<u32>> = resp.hidden.into_iter().map(|h| h.indices).collect();
                let output = resp.output.map(|o| o.indices).unwrap_or_default();

                return (
                    StatusCode::OK,
                    Json(json!({
                        "network_id": resp.network_id,
                        "sensory": { "indices": sensory },
                        "hidden": hidden.into_iter().map(|indices| json!({ "indices": indices })).collect::<Vec<_>>(),
                        "output": { "indices": output },
                        "source": target_addr,
                    })),
                )
                    .into_response();
            }
            Err(e) => {
                last_error = format!("activity failed via {}: {}", target_addr, e);
            }
        }
    }

    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({ "error": last_error })),
    )
        .into_response()
}

async fn update_network(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<UpdateNetworkPayload>,
) -> impl IntoResponse {
    if let Some(resp) = forbid_shared_cluster_api(state.as_ref()) {
        return resp;
    }
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
                .into_response();
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
                grpc_status_to_http(&e),
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
    if let Some(resp) = forbid_shared_cluster_api(state.as_ref()) {
        return resp;
    }
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
                .into_response();
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
                .into_response();
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

async fn export_snapshot_payload(
    network_id: String,
    snapshot_json: String,
    format: String,
) -> axum::response::Response {
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
                .into_response();
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

    let script_path = {
        let direct = std::path::PathBuf::from(format!("tools/{}", script));
        if direct.exists() {
            direct
        } else {
            std::path::PathBuf::from(format!("tools/{}c", script))
        }
    };

    let status = tokio::process::Command::new("python3")
        .arg(script_path)
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

async fn export(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ExportQuery>,
) -> impl IntoResponse {
    if let Some(resp) = forbid_shared_cluster_api(state.as_ref()) {
        return resp;
    }
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
                .into_response();
        }
    };

    let target_addrs = match resolve_network_addrs(addr.clone(), &network_id, None).await {
        Ok(addrs) => addrs,
        Err(resp) => return resp.into_response(),
    };

    let mut snapshot_json: Option<String> = None;
    let mut last_error = String::from("no candidate target attempted");
    for target_addr in target_addrs {
        let mut client = match connect_cluster_client(target_addr.clone()).await {
            Ok(client) => client,
            Err(e) => {
                last_error = format!("connect failed via {}: {}", target_addr, e);
                continue;
            }
        };

        match client
            .get_network_snapshot(Request::new(NetworkSnapshotRequest {
                network_id: network_id.clone(),
            }))
            .await
        {
            Ok(resp) => {
                snapshot_json = Some(resp.into_inner().snapshot_json);
                break;
            }
            Err(e) => {
                last_error = format!("snapshot failed via {}: {}", target_addr, e);
            }
        }
    }

    let Some(snapshot_json) = snapshot_json else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": last_error })),
        )
            .into_response();
    };

    export_snapshot_payload(network_id, snapshot_json, query.format.to_lowercase()).await
}

async fn export_runtime_workspace(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(workspace_id): Path<String>,
    Query(query): Query<ExportQuery>,
) -> impl IntoResponse {
    let owner = match resolve_runtime_workspace_owner(&user, query.owner.as_deref()) {
        Ok(owner) => owner,
        Err(response) => return response,
    };
    let snapshot = match state
        .runtime
        .workspace_snapshot(&owner, &workspace_id)
        .await
    {
        Ok(snapshot) => snapshot,
        Err(err) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": err.to_string() })),
            )
                .into_response();
        }
    };

    export_snapshot_payload(
        snapshot.workspace_id,
        snapshot.snapshot_json,
        query.format.to_lowercase(),
    )
    .await
}

fn runtime_workspace_scope_owners(user: &AuthUser) -> Vec<String> {
    let mut owners = vec![user.username.clone()];
    if user.is_admin
        && !owners
            .iter()
            .any(|owner| owner.eq_ignore_ascii_case("system"))
    {
        owners.push("system".to_string());
    }
    owners
}

fn resolve_runtime_workspace_owner(
    user: &AuthUser,
    requested_owner: Option<&str>,
) -> Result<String, axum::response::Response> {
    let owner = requested_owner.unwrap_or_default().trim();
    if owner.is_empty() || owner.eq_ignore_ascii_case(user.username.trim()) {
        return Ok(user.username.clone());
    }
    if user.is_admin && owner.eq_ignore_ascii_case("system") {
        return Ok("system".to_string());
    }
    Err((
        StatusCode::FORBIDDEN,
        Json(json!({ "error": "workspace owner access denied" })),
    )
        .into_response())
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
    decode_spike_hex_payload(trimmed).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": e.to_string() })),
        )
    })
}

async fn fetch_network_config(
    target_addr: &str,
    network_id: &str,
) -> Result<NetworkConfig, ApiError> {
    let mut client = connect_cluster_client(target_addr.to_string())
        .await
        .map_err(|e| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": format!("connect failed: {}", e) })),
            )
        })?;
    let snapshot_json = client
        .get_network_snapshot(Request::new(NetworkSnapshotRequest {
            network_id: network_id.to_string(),
        }))
        .await
        .map_err(|e| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": format!("snapshot failed: {}", e) })),
            )
        })?
        .into_inner()
        .snapshot_json;

    if let Ok(snapshot) = decode_snapshot_with_profile_backfill(&snapshot_json) {
        Ok(snapshot.net)
    } else {
        serde_json::from_str::<NetworkConfig>(&snapshot_json).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("failed to parse network snapshot: {}", e) })),
            )
        })
    }
}

async fn build_aer_batch(
    target_addr: &str,
    network_id: String,
    step_index: i64,
    time_ms: Option<f32>,
    dt_ms: Option<f32>,
    aer_base: u32,
    is_backward: bool,
    aer_payload_hex: Option<String>,
    spike_indices: Option<Vec<u32>>,
    input_values: Option<Vec<f32>>,
    spike_io: Option<SpikeIoConfig>,
) -> Result<SpikeBatch, ApiError> {
    let aer_payload = decode_hex_payload(aer_payload_hex)?;
    let spike_indices = spike_indices.unwrap_or_default();
    let has_input_values = input_values
        .as_ref()
        .map(|values| !values.is_empty())
        .unwrap_or(false);
    if aer_payload.is_empty() && spike_indices.is_empty() && !has_input_values {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "provide aer_payload_hex, spike_indices, or input_values"
            })),
        ));
    }

    if has_input_values {
        let net_cfg = fetch_network_config(target_addr, &network_id).await?;
        let mut combined = spikes_from_transport(
            &aer_payload,
            aer_base,
            &spike_indices,
            net_cfg.num_sensory_neurons,
        )
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": e.to_string() })),
            )
        })?;
        let mut encoded = vec![0i8; net_cfg.num_sensory_neurons];
        let dt_ms = dt_ms.unwrap_or(1.0).max(0.001);
        let time_ms = time_ms.unwrap_or_else(|| step_index.max(0) as f32 * dt_ms);
        let io_cfg = spike_io.unwrap_or_else(|| net_cfg.spike_io.clone());
        encode_network_inputs_with(
            &io_cfg,
            net_cfg.num_sensory_neurons,
            net_cfg.num_output_neurons,
            input_values.as_deref().unwrap_or(&[]),
            &mut encoded,
            fastrand::f32,
            TemporalEncodingContext {
                step_index: step_index.max(0) as usize,
                time_ms,
                dt_ms,
            },
        );
        for (dst, src) in combined.iter_mut().zip(encoded.iter()) {
            if *src != 0 {
                *dst = 1;
            }
        }
        let exchange = encode_exchange((time_ms.max(0.0) * 1000.0) as u64, aer_base, &combined);
        return Ok(SpikeBatch {
            network_id,
            layer_index: EXTERNAL_SENSORY_LAYER_INDEX,
            step_index,
            spike_indices: exchange.spike_indices,
            is_backward,
            aer_payload: exchange.aer_payload,
            aer_base: exchange.aer_base,
        });
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

const LLM_MIRROR_MAX_TEXT_CHARS: usize = 8192;
const LLM_MIRROR_MAX_SPIKES: usize = 4096;
const LLM_MIRROR_DEFAULT_SPIKES: usize = 128;

async fn llm_mirror(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Json(payload): Json<LlmMirrorPayload>,
) -> impl IntoResponse {
    let request_id = payload.request_id.trim().to_string();
    if request_id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "request_id is required" })),
        )
            .into_response();
    }
    let conversation_id = payload
        .conversation_id
        .trim()
        .to_string()
        .chars()
        .take(256)
        .collect::<String>();
    if conversation_id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "conversation_id is required" })),
        )
            .into_response();
    }
    let workflow = payload.workflow.trim().to_string();
    if workflow.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "workflow is required" })),
        )
            .into_response();
    }
    let role = payload.role.trim().to_string();
    if role.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "role is required" })),
        )
            .into_response();
    }
    let text = truncate_mirror_text(&compact_mirror_text(payload.text.as_str()));
    if text.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "text is required" })),
        )
            .into_response();
    }
    let system = payload
        .system
        .as_deref()
        .map(compact_mirror_text)
        .map(|value| truncate_mirror_text(&value))
        .filter(|value| !value.is_empty());
    let prompt_text = payload
        .prompt_text
        .as_deref()
        .map(compact_mirror_text)
        .map(|value| truncate_mirror_text(&value))
        .filter(|value| !value.is_empty());
    let provider = normalize_optional_text(payload.provider.as_deref());
    let model = normalize_optional_text(payload.model.as_deref());
    let request_category = normalize_optional_text(payload.request_category.as_deref());
    let sensory_spikes = normalise_mirror_spikes(
        payload.sensory_spikes.clone(),
        payload.aer_base,
        payload.aer_payload_hex.as_str(),
        text.as_str(),
    );
    let exchange_spikes = sensory_spikes
        .iter()
        .map(|value| if *value > 0 { 1i8 } else { 0i8 })
        .collect::<Vec<_>>();
    let exchange = encode_exchange(
        mirror_now_ms().saturating_mul(1000),
        payload.aer_base,
        &exchange_spikes,
    );
    let aer_payload_hex = hex::encode(&exchange.aer_payload);
    let spike_count = exchange.spike_indices.len();
    let stimulation =
        stimulate_llm_mirror(state.as_ref(), &payload, aer_payload_hex.as_str()).await;
    let candidate = build_llm_mirror_candidate(
        &payload.direction,
        payload.request_candidate_reply,
        text.as_str(),
        &exchange,
        sensory_spikes.len(),
        stimulation.accepted_batches,
    );
    let record = LlmMirrorRecord {
        recorded_at: mirror_now_ms(),
        actor: user.username.clone(),
        actor_role: user.role.clone(),
        actor_groups: user.groups.clone(),
        request_id: request_id.clone(),
        conversation_id: conversation_id.clone(),
        workflow,
        role,
        direction: payload.direction,
        provider,
        model,
        request_category,
        system,
        prompt_text,
        text: text.clone(),
        message_roles: payload.message_roles,
        aer_base: payload.aer_base,
        output_base: payload.output_base,
        aer_payload_hex: aer_payload_hex.clone(),
        sensory_spikes: sensory_spikes.clone(),
        stimulation: LlmMirrorStimulusResponse {
            attempted: stimulation.attempted,
            accepted_batches: stimulation.accepted_batches,
            target: stimulation.target.clone(),
            network_id: stimulation.network_id.clone(),
            error: stimulation.error.clone(),
        },
        candidate: candidate.as_ref().map(clone_llm_mirror_candidate),
    };
    if let Err(error) = persist_llm_mirror_record(
        state.runtime.root_dir().to_path_buf(),
        conversation_id.as_str(),
        request_id.as_str(),
        payload.direction,
        record,
    )
    .await
    {
        eprintln!(
            "[warn] failed to persist mirrored LLM exchange request_id={} error={}",
            request_id, error
        );
    }

    (
        StatusCode::OK,
        Json(LlmMirrorResponseBody {
            accepted: true,
            request_id,
            conversation_id,
            direction: payload.direction,
            text_chars: text.chars().count(),
            spike_count,
            aer_payload_hex,
            candidate,
            stimulation: Some(stimulation),
        }),
    )
        .into_response()
}

fn compact_mirror_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_mirror_text(value: &str) -> String {
    value.chars().take(LLM_MIRROR_MAX_TEXT_CHARS).collect()
}

fn normalize_optional_text(value: Option<&str>) -> Option<String> {
    value
        .map(compact_mirror_text)
        .map(|value| truncate_mirror_text(&value))
        .filter(|value| !value.is_empty())
}

fn normalise_mirror_spikes(
    spikes: Vec<u8>,
    aer_base: u32,
    aer_payload_hex: &str,
    text: &str,
) -> Vec<u8> {
    let sanitized = spikes
        .into_iter()
        .take(LLM_MIRROR_MAX_SPIKES)
        .map(|value| if value > 0 { 1u8 } else { 0u8 })
        .collect::<Vec<_>>();
    if sanitized.iter().any(|value| *value > 0) {
        sanitized
    } else {
        if let Ok(aer_payload) = decode_spike_hex_payload(aer_payload_hex) {
            if let Ok(decoded) =
                spikes_from_transport(&aer_payload, aer_base, &[], LLM_MIRROR_DEFAULT_SPIKES)
            {
                let decoded = decoded
                    .into_iter()
                    .map(|value| if value > 0 { 1u8 } else { 0u8 })
                    .collect::<Vec<_>>();
                if decoded.iter().any(|value| *value > 0) {
                    return decoded;
                }
            }
        }
        text_to_mirror_spikes(text, LLM_MIRROR_DEFAULT_SPIKES)
    }
}

fn text_to_mirror_spikes(text: &str, sensory_size: usize) -> Vec<u8> {
    let sensory_size = sensory_size.clamp(32, LLM_MIRROR_MAX_SPIKES);
    let mut spikes = vec![0u8; sensory_size];
    let compact = compact_mirror_text(text).to_ascii_lowercase();
    let bytes = compact.as_bytes();
    let len = bytes.len().max(1);
    for (index, byte) in bytes.iter().enumerate() {
        let primary = ((*byte as usize) + index * 17 + len * 29) % sensory_size;
        spikes[primary] = 1;
        if index > 0 {
            let previous = bytes[index - 1] as usize;
            let secondary =
                ((*byte as usize) * 31 + previous + index * 11 + len * 7) % sensory_size;
            spikes[secondary] = 1;
        }
    }
    spikes
}

fn build_llm_mirror_candidate(
    direction: &LlmMirrorDirection,
    request_candidate_reply: bool,
    text: &str,
    exchange: &aarnn_rust::spike_io::transport::SpikeExchange,
    sensory_size: usize,
    accepted_batches: usize,
) -> Option<LlmMirrorCandidateResponse> {
    if !request_candidate_reply || !matches!(direction, LlmMirrorDirection::Output) {
        return None;
    }
    let density = exchange.spike_indices.len() as f64 / sensory_size.max(1) as f64;
    let confidence =
        (0.16 + density * 0.24 + if accepted_batches > 0 { 0.1 } else { 0.0 }).clamp(0.16, 0.55);
    Some(LlmMirrorCandidateResponse {
        reply_text: Some(text.to_string()),
        confidence: Some((confidence * 1000.0).round() / 1000.0),
        usable: true,
        source: Some(
            if accepted_batches > 0 {
                "stimulated_transport_echo"
            } else {
                "transport_mirror_echo"
            }
            .to_string(),
        ),
        output_spike_indices: exchange.spike_indices.clone(),
        output_aer_payload_hex: Some(hex::encode(&exchange.aer_payload)),
    })
}

fn clone_llm_mirror_candidate(
    candidate: &LlmMirrorCandidateResponse,
) -> LlmMirrorCandidateResponse {
    LlmMirrorCandidateResponse {
        reply_text: candidate.reply_text.clone(),
        confidence: candidate.confidence,
        usable: candidate.usable,
        source: candidate.source.clone(),
        output_spike_indices: candidate.output_spike_indices.clone(),
        output_aer_payload_hex: candidate.output_aer_payload_hex.clone(),
    }
}

async fn stimulate_llm_mirror(
    state: &AppState,
    payload: &LlmMirrorPayload,
    aer_payload_hex: &str,
) -> LlmMirrorStimulusResponse {
    let network_id = payload
        .network_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let Some(network_id) = network_id else {
        return LlmMirrorStimulusResponse {
            attempted: false,
            accepted_batches: 0,
            target: None,
            network_id: None,
            error: None,
        };
    };

    let mut response = LlmMirrorStimulusResponse {
        attempted: true,
        accepted_batches: 0,
        target: None,
        network_id: Some(network_id.clone()),
        error: None,
    };

    let orchestrator_addr = match resolve_addr_or_default(None, state.default_orchestrator.clone())
    {
        Ok(addr) => addr,
        Err(err) => {
            response.error = Some(api_error_message(err));
            return response;
        }
    };

    let node_id = payload
        .node_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let target_addr = if node_id.is_some() {
        match resolve_network_addr(orchestrator_addr.clone(), &network_id, node_id).await {
            Ok(addr) => addr,
            Err(err) => {
                response.error = Some(api_error_message(err));
                return response;
            }
        }
    } else {
        orchestrator_addr
    };
    response.target = Some(target_addr.clone());

    let batch = match build_aer_batch(
        &target_addr,
        network_id.clone(),
        0,
        None,
        None,
        payload.aer_base,
        matches!(payload.direction, LlmMirrorDirection::Output),
        Some(aer_payload_hex.to_string()),
        None,
        None,
        None,
    )
    .await
    {
        Ok(batch) => batch,
        Err(err) => {
            response.error = Some(api_error_message(err));
            return response;
        }
    };

    match send_aer_batches(target_addr, vec![batch]).await {
        Ok(accepted) => {
            response.accepted_batches = accepted;
            response
        }
        Err(err) => {
            response.error = Some(api_error_message(err));
            response
        }
    }
}

async fn persist_llm_mirror_record(
    runtime_root: PathBuf,
    conversation_id: &str,
    request_id: &str,
    direction: LlmMirrorDirection,
    record: LlmMirrorRecord,
) -> anyhow::Result<()> {
    let safe_conversation = sanitize_mirror_segment(conversation_id, "conversation");
    let safe_request = sanitize_mirror_segment(request_id, "request");
    let path = runtime_root
        .join("llm_mirror")
        .join(safe_conversation)
        .join(format!(
            "{}-{}-{}.json",
            mirror_now_ms(),
            safe_request,
            mirror_direction_label(direction)
        ));
    tokio::task::spawn_blocking(move || write_json_pretty(&path, &record))
        .await
        .context("llm mirror persistence task failed")?
}

fn sanitize_mirror_segment(raw: &str, fallback: &str) -> String {
    let sanitized = raw
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches(|ch| ch == '-' || ch == '.')
        .to_ascii_lowercase();
    if sanitized.is_empty() {
        fallback.to_string()
    } else {
        sanitized
    }
}

fn mirror_direction_label(direction: LlmMirrorDirection) -> &'static str {
    match direction {
        LlmMirrorDirection::Input => "input",
        LlmMirrorDirection::Output => "output",
    }
}

fn mirror_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn api_error_message(error: ApiError) -> String {
    let (status, Json(payload)) = error;
    payload
        .get("error")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            payload
                .get("details")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| status.to_string())
}

async fn aer_inject(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<AerInjectPayload>,
) -> impl IntoResponse {
    if let Some(resp) = forbid_shared_cluster_api(state.as_ref()) {
        return resp;
    }
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
        &target_addr,
        payload.network_id.clone(),
        payload.step_index.unwrap_or(0),
        payload.time_ms,
        payload.dt_ms,
        payload.aer_base.unwrap_or(0),
        payload.is_backward.unwrap_or(false),
        payload.aer_payload_hex,
        payload.spike_indices,
        payload.input_values,
        payload.spike_io,
    )
    .await
    {
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
    if let Some(resp) = forbid_shared_cluster_api(state.as_ref()) {
        return resp;
    }
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
                    .into_response();
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
                        .into_response();
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
                &orchestrator_addr,
                frame_network_id,
                frame.step_index.or(query.step_index).unwrap_or(0),
                frame.time_ms,
                frame.dt_ms,
                frame.aer_base.or(query.aer_base).unwrap_or(0),
                frame.is_backward.or(query.is_backward).unwrap_or(false),
                frame.aer_payload_hex,
                frame.spike_indices,
                frame.input_values,
                frame.spike_io,
            )
            .await
            {
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
                    .into_response();
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
                &orchestrator_addr,
                frame_network_id,
                frame.step_index.or(query.step_index).unwrap_or(0),
                frame.time_ms,
                frame.dt_ms,
                frame.aer_base.or(query.aer_base).unwrap_or(0),
                frame.is_backward.or(query.is_backward).unwrap_or(false),
                frame.aer_payload_hex,
                frame.spike_indices,
                frame.input_values,
                frame.spike_io,
            )
            .await
            {
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
                .into_response();
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

async fn resolve_network_addrs(
    orchestrator_addr: String,
    network_id: &str,
    node_id: Option<String>,
) -> Result<Vec<String>, ApiError> {
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
            b.0.cmp(&a.0)
                .then_with(|| b.1.cmp(&a.1))
                .then_with(|| a.2.cmp(&b.2))
        });

        for (_, _, nid) in ranked {
            if let Some(node) = status
                .nodes
                .iter()
                .find(|n| n.node_id == nid && n.active_networks.iter().any(|id| id == network_id))
            {
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
        let a_capacity = a
            .resources
            .as_ref()
            .map(|r| r.capacity_score)
            .unwrap_or(0.0);
        let b_capacity = b
            .resources
            .as_ref()
            .map(|r| r.capacity_score)
            .unwrap_or(0.0);

        b_neurons
            .cmp(&a_neurons)
            .then_with(|| {
                b_capacity
                    .partial_cmp(&a_capacity)
                    .unwrap_or(Ordering::Equal)
            })
            .then_with(|| a.node_id.cmp(&b.node_id))
    });
    for node in active_nodes {
        push_candidate(node.address.clone());
    }

    push_candidate(orchestrator_addr);

    if candidates.is_empty() {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "no available node address for requested network" })),
        ));
    }
    Ok(candidates)
}

async fn resolve_network_addr(
    orchestrator_addr: String,
    network_id: &str,
    node_id: Option<String>,
) -> Result<String, ApiError> {
    let targets = resolve_network_addrs(orchestrator_addr, network_id, node_id).await?;
    match targets.first() {
        Some(addr) => Ok(addr.clone()),
        None => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "no candidate target address resolved" })),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_access_requirement_maps_runtime_routes() {
        assert_eq!(
            api_access_requirement(&Method::GET, "/api/tokens"),
            Some(AccessRequirement::aarnn_request())
        );
        assert_eq!(
            api_access_requirement(&Method::GET, "/api/runtime/status"),
            Some(AccessRequirement::aarnn_observe())
        );
        assert_eq!(
            api_access_requirement(&Method::POST, "/api/runtime/workspaces"),
            Some(AccessRequirement::aarnn_use())
        );
        assert_eq!(
            api_access_requirement(&Method::POST, "/api/llm/mirror"),
            Some(AccessRequirement::aarnn_use())
        );
        assert_eq!(
            api_access_requirement(&Method::DELETE, "/api/runtime/workspaces/demo"),
            Some(AccessRequirement::aarnn_control())
        );
        assert_eq!(
            api_access_requirement(&Method::GET, "/api/runtime/workspaces/demo/snapshot"),
            Some(AccessRequirement::aarnn_observe())
        );
    }

    #[test]
    fn api_access_requirement_allows_public_routes() {
        assert!(is_public_api_path("/api/openapi.json"));
        assert_eq!(api_access_requirement(&Method::POST, "/api/logout"), None);
        assert_eq!(api_access_requirement(&Method::GET, "/api/me"), None);
    }

    #[test]
    fn workspace_control_requirement_escalates_destructive_actions() {
        assert_eq!(
            workspace_control_requirement(WorkspaceControlAction::Start),
            AccessRequirement::aarnn_use()
        );
        assert_eq!(
            workspace_control_requirement(WorkspaceControlAction::Repeat),
            AccessRequirement::aarnn_use()
        );
        assert_eq!(
            workspace_control_requirement(WorkspaceControlAction::Stop),
            AccessRequirement::aarnn_control()
        );
        assert_eq!(
            workspace_control_requirement(WorkspaceControlAction::Reset),
            AccessRequirement::aarnn_control()
        );
        assert_eq!(
            workspace_control_requirement(WorkspaceControlAction::New),
            AccessRequirement::aarnn_control()
        );
    }

    #[test]
    fn runtime_workspace_scope_includes_system_for_admins() {
        let user = AuthUser {
            username: "pbisaacs".to_string(),
            is_admin: true,
            ..AuthUser::default()
        };
        assert_eq!(
            runtime_workspace_scope_owners(&user),
            vec!["pbisaacs".to_string(), "system".to_string()]
        );
    }

    #[test]
    fn runtime_workspace_owner_resolution_allows_admin_system_access() {
        let user = AuthUser {
            username: "pbisaacs".to_string(),
            is_admin: true,
            ..AuthUser::default()
        };
        assert_eq!(
            resolve_runtime_workspace_owner(&user, Some("system")).unwrap(),
            "system"
        );
        assert_eq!(
            resolve_runtime_workspace_owner(&user, Some("pbisaacs")).unwrap(),
            "pbisaacs"
        );
    }

    #[test]
    fn runtime_workspace_owner_resolution_rejects_non_admin_system_access() {
        let user = AuthUser {
            username: "alice".to_string(),
            is_admin: false,
            ..AuthUser::default()
        };
        let response = resolve_runtime_workspace_owner(&user, Some("system")).unwrap_err();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn llm_mirror_candidate_remains_low_confidence_echo() {
        let exchange = encode_exchange(1000, 4096, &[0, 1, 1, 0, 1, 0]);
        let candidate = build_llm_mirror_candidate(
            &LlmMirrorDirection::Output,
            true,
            "AARNN mirrored reply",
            &exchange,
            6,
            1,
        )
        .expect("candidate");
        assert!(candidate.usable);
        assert_eq!(
            candidate.reply_text.as_deref(),
            Some("AARNN mirrored reply")
        );
        assert!(candidate.confidence.unwrap_or_default() < 0.6);
    }
}
