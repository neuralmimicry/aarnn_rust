use axum::{
    extract::{Form, Query, State},
    http::{HeaderValue, StatusCode},
    middleware,
    response::{IntoResponse, Redirect},
    routing::{get, post},
    Extension, Json, Router,
};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tonic::Request;
use tokio::sync::RwLock;
use tokio::fs;
use aarnn_rust::distributed::proto::{
    distributed_neuromorphic_client::DistributedNeuromorphicClient, network_update_request,
    control_update, ConfigUpdate, ControlUpdate, NetworkActivityRequest, NetworkSnapshotRequest,
    NetworkUpdateRequest, StatusRequest,
};
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use openidconnect::{
    core::{CoreAuthenticationFlow, CoreClient, CoreIdToken, CoreProviderMetadata, CoreUserInfoClaims},
    reqwest,
    AccessToken, AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce,
    EndpointMaybeSet, EndpointNotSet, EndpointSet, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl,
    Scope, TokenResponse,
};
use argon2::password_hash::rand_core::OsRng;

type OidcClient = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

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
        args.oidc_redirect_url = env_opt("NM_OIDC_REDIRECT_URI")
            .or_else(|| env_opt("NM_OIDC_REDIRECT_URL"));
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
            CoreProviderMetadata::discover_async(IssuerUrl::new(issuer.clone())?, &http_client).await?;
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
        .route("/update_network", post(update_network))
        .route("/control_network", post(control_network))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            api_auth_middleware,
        ));

    let app = Router::new()
        .route("/", get(index))
        .route("/app.js", get(app_js))
        .route("/style.css", get(style_css))
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
    if matches!(path, "/api/auth/mode" | "/api/login" | "/api/signup" | "/api/me") {
        return next.run(req).await;
    }
    let jar = CookieJar::from_headers(req.headers());
    if let Some(user) = session_user(&state, &jar).await {
        req.extensions_mut().insert(AuthUser { username: user });
        return next.run(req).await;
    }
    (StatusCode::UNAUTHORIZED, Json(json!({ "error": "unauthorized" }))).into_response()
}

async fn auth_mode_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Json(AuthModeResponse {
        mode: state.auth.mode.as_str().to_string(),
        allow_signup: state.auth.allow_signup,
    })
}

async fn me(State(state): State<Arc<AppState>>, jar: CookieJar) -> axum::response::Response {
    if state.auth.mode == AuthMode::None {
        return Json(json!({ "authenticated": false, "mode": "none" })).into_response();
    }
    if let Some(user) = session_user(&state, &jar).await {
        return Json(json!({ "authenticated": true, "username": user })).into_response();
    }
    (StatusCode::UNAUTHORIZED, Json(json!({ "error": "unauthorized" }))).into_response()
}

async fn login(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Json(payload): Json<LoginPayload>,
) -> impl IntoResponse {
    if state.auth.mode != AuthMode::Local {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "local auth disabled" }))).into_response();
    }
    let username = payload.username.trim();
    let password = payload.password.trim();
    if username.is_empty() || password.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "missing credentials" }))).into_response();
    }
    let users = state.users.read().await;
    let user = match users.find_by_username(username) {
        Some(u) => u,
        None => {
            return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "invalid credentials" }))).into_response();
        }
    };
    let hash = match user.password_hash.as_deref() {
        Some(h) => h,
        None => {
            return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "invalid credentials" }))).into_response();
        }
    };
    if !verify_password(hash, password) {
        return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "invalid credentials" }))).into_response();
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
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "local auth disabled" }))).into_response();
    }
    if !state.auth.allow_signup {
        return (StatusCode::FORBIDDEN, Json(json!({ "error": "signup disabled" }))).into_response();
    }
    let username = payload.username.trim();
    let password = payload.password.trim();
    if username.is_empty() || password.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "missing credentials" }))).into_response();
    }

    let mut users = state.users.write().await;
    if users.find_by_username(username).is_some() {
        return (StatusCode::CONFLICT, Json(json!({ "error": "user exists" }))).into_response();
    }
    let hash = match hash_password(password) {
        Ok(h) => h,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response();
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
        return Json(json!({ "config": rec.config.clone().unwrap_or_else(|| json!({})) })).into_response();
    }
    (StatusCode::NOT_FOUND, Json(json!({ "error": "user not found" }))).into_response()
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
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": "oidc disabled" }))).into_response();
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
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": "oidc disabled" }))).into_response();
        }
    };
    let pending = {
        let mut pending = oidc.pending.write().await;
        pending.remove(&query.state)
    };
    let pending = match pending {
        Some(p) => p,
        None => {
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid state" }))).into_response();
        }
    };

    let token_req = match oidc.client.exchange_code(AuthorizationCode::new(query.code)) {
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
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": format!("token exchange failed: {}", e) }))).into_response();
        }
    };

    let id_token = match token_resp.id_token() {
        Some(token) => token,
        None => {
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": "missing id_token" }))).into_response();
        }
    };
    let claims = match id_token.claims(&oidc.client.id_token_verifier(), &pending.nonce) {
        Ok(c) => c,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": format!("invalid id_token: {}", e) }))).into_response();
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
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "oidc disabled" }))).into_response();
    }
    let oidc = match state.auth.oidc.as_ref() {
        Some(oidc) => oidc.clone(),
        None => {
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": "oidc disabled" }))).into_response();
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
            Ok(req) => {
                req.request_async(&oidc.http_client)
                    .await
                    .ok()
            }
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
                    return (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid id_token" }))).into_response();
                }
            };
            let verifier = oidc.client.id_token_verifier();
            let claims = match id_token.claims(&verifier, |_: Option<&Nonce>| Ok(())) {
                Ok(claims) => claims,
                Err(_) => {
                    return (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid id_token" }))).into_response();
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
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": "missing token claims" }))).into_response();
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
            .or_else(|| email.as_ref().and_then(|e| e.split('@').next().map(|s| s.to_string())))
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
        Err(code) => return (code, Json(json!({ "error": "missing orchestrator address" }))).into_response(),
    };

    let mut client = match DistributedNeuromorphicClient::connect(addr.clone()).await {
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
                "temperature_c": res.map(|r| r.temperature_c).unwrap_or(-1.0),
                "ga_running": res.map(|r| r.ga_running).unwrap_or(false),
                "ga_generation": res.map(|r| r.ga_generation).unwrap_or(0),
                "ga_best_fitness": res.map(|r| r.ga_best_fitness).unwrap_or(0.0),
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
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "missing network_id" }))).into_response();
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
        Err(code) => return (code, Json(json!({ "error": "missing orchestrator address" }))).into_response(),
    };

    let target_addr = match resolve_network_addr(addr.clone(), &network_id, query.node_id.clone()).await {
        Ok(a) => a,
        Err(resp) => return resp.into_response(),
    };

    let mut client = match DistributedNeuromorphicClient::connect(target_addr.clone()).await {
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
        .get_network_snapshot(Request::new(NetworkSnapshotRequest { network_id: network_id.clone() }))
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
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "missing network_id" }))).into_response();
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
        Err(code) => return (code, Json(json!({ "error": "missing orchestrator address" }))).into_response(),
    };

    let target_addr = match resolve_network_addr(addr.clone(), &network_id, query.node_id.clone()).await {
        Ok(a) => a,
        Err(resp) => return resp.into_response(),
    };

    let mut client = match DistributedNeuromorphicClient::connect(target_addr.clone()).await {
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
        .get_network_activity(Request::new(NetworkActivityRequest { network_id: network_id.clone() }))
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
            return (code, Json(json!({ "error": "missing orchestrator address" }))).into_response()
        }
    };

    let mut client = match DistributedNeuromorphicClient::connect(addr.clone()).await {
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
        config_json: payload
            .config_json
            .unwrap_or_default()
            .as_bytes()
            .to_vec(),
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
            return (code, Json(json!({ "error": "missing orchestrator address" }))).into_response()
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

    let mut client = match DistributedNeuromorphicClient::connect(addr.clone()).await {
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
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "missing network_id" }))).into_response();
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
        Err(code) => return (code, Json(json!({ "error": "missing orchestrator address" }))).into_response(),
    };

    let target_addr = match resolve_network_addr(addr.clone(), &network_id, None).await {
        Ok(a) => a,
        Err(resp) => return resp.into_response(),
    };

    let mut client = match DistributedNeuromorphicClient::connect(target_addr.clone()).await {
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
        .get_network_snapshot(Request::new(NetworkSnapshotRequest { network_id: network_id.clone() }))
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
        _ => return (StatusCode::BAD_REQUEST, Json(json!({ "error": "unsupported format" }))).into_response(),
    };

    let tmp_in = std::env::temp_dir().join(format!("network_{}.json", network_id));
    let tmp_out = std::env::temp_dir().join(format!("exported_{}.{}", network_id, ext));

    if let Err(e) = fs::write(&tmp_in, snapshot_json).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("write temp failed: {}", e) }))).into_response();
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
        Ok(s) if s.success() => {
            match fs::read(&tmp_out).await {
                Ok(content) => {
                    let mut headers = axum::http::HeaderMap::new();
                    headers.insert(axum::http::header::CONTENT_TYPE, HeaderValue::from_static("application/octet-stream"));
                    headers.insert(
                        axum::http::header::CONTENT_DISPOSITION,
                        HeaderValue::from_str(&format!("attachment; filename=\"exported_{}.{}\"", network_id, ext)).unwrap()
                    );
                    (StatusCode::OK, headers, content).into_response()
                }
                Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("read output failed: {}", e) }))).into_response(),
            }
        }
        Ok(s) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("export tool failed with status {}", s) }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("failed to run export tool: {}", e) }))).into_response(),
    }
}

type ApiError = (StatusCode, Json<serde_json::Value>);

async fn resolve_network_addr(
    orchestrator_addr: String,
    network_id: &str,
    node_id: Option<String>,
) -> Result<String, ApiError> {
    let mut client = DistributedNeuromorphicClient::connect(orchestrator_addr.clone())
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

    let candidate = if let Some(node_id) = node_id {
        status
            .nodes
            .iter()
            .find(|n| n.node_id == node_id)
            .map(|n| n.address.clone())
    } else {
        status
            .nodes
            .iter()
            .find(|n| n.active_networks.iter().any(|id| id == network_id))
            .map(|n| n.address.clone())
    };

    let target = candidate.unwrap_or(orchestrator_addr);
    let target = if target.starts_with("http://") || target.starts_with("https://") {
        target
    } else {
        format!("http://{}", target)
    };
    Ok(target)
}
