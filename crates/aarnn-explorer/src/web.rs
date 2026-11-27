// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Loopback browser UI and versioned job API `/api/explorer/v1` (feature
//! `web-ui`). Rust owns every analysis and experiment; the browser presents.
//!
//! Security:
//! - binds loopback only;
//! - a random session token (from the OS) is printed once in the start URL.
//!   `GET /?session=TOKEN` exchanges it for an HttpOnly SameSite=Strict
//!   cookie;
//! - mutations need the `x-explorer-csrf` header and, when the browser sends
//!   one, a matching `Origin`;
//! - request bodies are limited to 256 KiB;
//! - there is no remote listener, no credential, and no route to physical
//!   actuation.
//!
//! Expensive work (baseline, analyse, experiment, optimise, validate, export)
//! runs as background jobs with states, monotonic event cursors and
//! cancellation, so no handler blocks on a simulation.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::{DefaultBodyLimit, Path as UrlPath, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde_json::{Value, json};

use crate::error::{ExplorerError, Result};
use crate::model::Topology;
use crate::pipeline::{self, Ctx};
use crate::store::{Project, RecordKind};

const INDEX: &str = include_str!("../web/index.html");
const APP_JS: &str = include_str!("../web/app.js");
const STYLE: &str = include_str!("../web/style.css");
const MAX_PAGE: usize = 2000;

struct JobEntry {
    id: String,
    kind: String,
    params: Value,
    state: String,
    result: Option<Value>,
    error: Option<String>,
    cancel: Arc<AtomicBool>,
    events: Vec<Value>,
}

struct App {
    project: PathBuf,
    token: String,
    csrf: String,
    origin: String,
    jobs: Mutex<BTreeMap<String, JobEntry>>,
    seq: AtomicU64,
}

type S = State<Arc<App>>;

fn random_hex(n: usize) -> Result<String> {
    let mut b = vec![0u8; n];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut b)?;
    Ok(hex::encode(b))
}

fn err(code: StatusCode, msg: impl Into<String>) -> Response {
    (code, axum::Json(json!({"error": msg.into()}))).into_response()
}

fn explorer_err(e: ExplorerError) -> Response {
    let code = match e {
        ExplorerError::InvalidInput(_) => StatusCode::BAD_REQUEST,
        ExplorerError::Unsupported(_) => StatusCode::NOT_IMPLEMENTED,
        ExplorerError::Conflict(_) => StatusCode::CONFLICT,
        ExplorerError::Budget(_) => StatusCode::PAYLOAD_TOO_LARGE,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (
        code,
        axum::Json(json!({"error": e.code(), "message": e.to_string()})),
    )
        .into_response()
}

fn cookie_ok(app: &App, h: &HeaderMap) -> bool {
    h.get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .any(|kv| kv.trim() == format!("explorer_session={}", app.token))
}

fn mutation_ok(app: &App, h: &HeaderMap) -> bool {
    let csrf = h.get("x-explorer-csrf").and_then(|v| v.to_str().ok()) == Some(app.csrf.as_str());
    let origin = h
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .is_none_or(|o| o == app.origin);
    cookie_ok(app, h) && csrf && origin
}

async fn index(State(app): S, Query(q): Query<BTreeMap<String, String>>, h: HeaderMap) -> Response {
    if q.get("session").is_some_and(|t| t == &app.token) {
        let mut r = (StatusCode::SEE_OTHER, "").into_response();
        r.headers_mut()
            .insert(header::LOCATION, HeaderValue::from_static("/"));
        if let Ok(c) = HeaderValue::from_str(&format!(
            "explorer_session={}; HttpOnly; SameSite=Strict; Path=/",
            app.token
        )) {
            r.headers_mut().insert(header::SET_COOKIE, c);
        }
        return r;
    }
    if !cookie_ok(&app, &h) {
        return (StatusCode::FORBIDDEN, axum::response::Html("<!doctype html><title>AARNN explorer</title><p>Open the session URL printed by <code>aarnn-explorer serve</code>.</p>".to_string())).into_response();
    }
    let page = INDEX.replace("__CSRF__", &app.csrf);
    let mut r = axum::response::Html(page).into_response();
    r.headers_mut().insert("content-security-policy", HeaderValue::from_static("default-src 'self'; style-src 'self'; script-src 'self'; connect-src 'self'; img-src 'self' data:"));
    r
}

async fn asset(State(app): S, UrlPath(name): UrlPath<String>, h: HeaderMap) -> Response {
    if !cookie_ok(&app, &h) {
        return err(StatusCode::FORBIDDEN, "session required");
    }
    let (body, ty) = match name.as_str() {
        "app.js" => (APP_JS, "text/javascript; charset=utf-8"),
        "style.css" => (STYLE, "text/css; charset=utf-8"),
        _ => return err(StatusCode::NOT_FOUND, "no such asset"),
    };
    ([(header::CONTENT_TYPE, ty)], body).into_response()
}

fn blocking<F: FnOnce() -> Result<Value> + Send + 'static>(
    f: F,
) -> impl std::future::Future<Output = Response> {
    async move {
        match tokio::task::spawn_blocking(f).await {
            Ok(Ok(v)) => axum::Json(v).into_response(),
            Ok(Err(e)) => explorer_err(e),
            Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        }
    }
}

macro_rules! guard {
    ($app:expr, $h:expr) => {
        if !cookie_ok(&$app, &$h) {
            return err(StatusCode::FORBIDDEN, "session required");
        }
    };
}

async fn status(State(app): S, h: HeaderMap) -> Response {
    guard!(app, h);
    let dir = app.project.clone();
    blocking(move || {
        let mut d = crate::doctor::report(Some(&dir));
        d["api"] = json!({"version": "v1", "max_page": MAX_PAGE});
        if let Ok(c) = crate::config::ExplorerConfig::parse(pipeline::DEFAULT_CONFIG) {
            d["budgets"] = json!({"profile": c.name, "resources": c.resources});
        }
        Ok(d)
    })
    .await
}

async fn index_records(State(app): S, h: HeaderMap) -> Response {
    guard!(app, h);
    let dir = app.project.clone();
    blocking(move || Ok(serde_json::to_value(Project::open(&dir)?.read_index()?)?)).await
}

fn load_topology(p: &Project, source: &str) -> Result<(Value, Topology)> {
    let rec: Value = p.get_json(&p.lookup(RecordKind::Source, source)?)?;
    let t: Topology = p.get_json(rec["topology_object"].as_str().unwrap_or(""))?;
    Ok((rec, t))
}

async fn sources(State(app): S, h: HeaderMap) -> Response {
    guard!(app, h);
    let dir = app.project.clone();
    blocking(move || {
        let p = Project::open(&dir)?;
        let idx = p.read_index()?;
        let mut out = Vec::new();
        for (name, hsh) in &idx.sources {
            let rec: Value = p.get_json(hsh)?;
            out.push(json!({"name": name, "record": hsh, "format": rec["format"], "capability": rec["capability"], "summary": rec["summary"], "input_sha256": rec["input_sha256"], "assumptions": rec["assumptions"], "limitations": rec["limitations"]}));
        }
        Ok(json!({"sources": out}))
    })
    .await
}

/// Paged topology: neurons and chemical/electrical synapses with exact
/// string IDs. `offset`/`limit` page neurons; synapses whose endpoints are
/// both on the page are included (bounded by MAX_PAGE).
async fn topology(
    State(app): S,
    Query(q): Query<BTreeMap<String, String>>,
    h: HeaderMap,
) -> Response {
    guard!(app, h);
    let dir = app.project.clone();
    blocking(move || {
        let p = Project::open(&dir)?;
        let src = q.get("source").cloned().or(p.default_value("source")?).ok_or_else(|| ExplorerError::InvalidInput("source".into()))?;
        let (_, t) = load_topology(&p, &src)?;
        let off: usize = q.get("offset").and_then(|x| x.parse().ok()).unwrap_or(0);
        let lim: usize = q.get("limit").and_then(|x| x.parse().ok()).unwrap_or(500).min(MAX_PAGE);
        let page: Vec<usize> = (off..(off + lim).min(t.neurons.len())).collect();
        let on: std::collections::BTreeSet<u32> = page.iter().map(|&i| i as u32).collect();
        let syn: Vec<Value> = t.synapses.iter().filter(|s| on.contains(&s.pre) && on.contains(&s.post)).take(MAX_PAGE * 4)
            .map(|s| json!({"id": s.id, "pre": t.neurons[s.pre as usize].id, "post": t.neurons[s.post as usize].id, "class": s.class, "weight": s.weight, "delay_ticks": s.delay_ticks}))
            .collect();
        Ok(json!({
            "source": src, "total_neurons": t.neurons.len(), "total_synapses": t.synapses.len(), "offset": off, "limit": lim,
            "neurons": page.iter().map(|&i| { let n = &t.neurons[i]; json!({"index": i, "id": n.id, "role": n.role, "type": n.type_name, "region": n.region, "graded": n.graded, "position": n.position, "position_kind": n.position_kind}) }).collect::<Vec<_>>(),
            "synapses": syn, "assumptions": t.assumptions, "limitations": t.limitations, "capability": t.capability,
            "display_note": "positions are procedural for designed circuits; the page is a bounded view, not a decimation of the analysis",
        }))
    })
    .await
}

/// Every incoming and outgoing synapse of one neuron (exact string IDs).
async fn neuron(
    State(app): S,
    Query(q): Query<BTreeMap<String, String>>,
    h: HeaderMap,
) -> Response {
    guard!(app, h);
    let dir = app.project.clone();
    blocking(move || {
        let p = Project::open(&dir)?;
        let src = q.get("source").cloned().or(p.default_value("source")?).ok_or_else(|| ExplorerError::InvalidInput("source".into()))?;
        let id = q.get("id").ok_or_else(|| ExplorerError::InvalidInput("id".into()))?;
        let (_, t) = load_topology(&p, &src)?;
        let i = t.index_of(id).ok_or_else(|| ExplorerError::InvalidInput(format!("no neuron {id}")))?;
        let n = &t.neurons[i as usize];
        let syn = |f: &dyn Fn(&crate::model::Synapse) -> bool| {
            t.synapses
                .iter()
                .filter(|s| f(s))
                .take(MAX_PAGE * 4)
                .map(|s| json!({"id": s.id, "pre": t.neurons[s.pre as usize].id, "post": t.neurons[s.post as usize].id, "class": s.class, "weight": s.weight, "delay_ticks": s.delay_ticks}))
                .collect::<Vec<_>>()
        };
        Ok(json!({"neuron": {"index": i, "id": n.id, "role": n.role, "type": n.type_name, "region": n.region, "graded": n.graded},
            "incoming": syn(&|s| s.post == i), "outgoing": syn(&|s| s.pre == i)}))
    })
    .await
}

/// Neuron search by ID, type or region substring (bounded).
async fn search(
    State(app): S,
    Query(q): Query<BTreeMap<String, String>>,
    h: HeaderMap,
) -> Response {
    guard!(app, h);
    let dir = app.project.clone();
    blocking(move || {
        let p = Project::open(&dir)?;
        let src = q.get("source").cloned().or(p.default_value("source")?).ok_or_else(|| ExplorerError::InvalidInput("source".into()))?;
        let needle = q.get("q").map(|x| x.to_lowercase()).unwrap_or_default();
        let (_, t) = load_topology(&p, &src)?;
        let hits: Vec<Value> = t
            .neurons
            .iter()
            .enumerate()
            .filter(|(_, n)| n.id.to_lowercase().contains(&needle) || n.type_name.to_lowercase().contains(&needle) || n.region.to_lowercase().contains(&needle))
            .take(200)
            .map(|(i, n)| json!({"index": i, "id": n.id, "role": n.role, "type": n.type_name, "region": n.region}))
            .collect();
        Ok(json!({"source": src, "q": needle, "hits": hits, "limit": 200}))
    })
    .await
}

async fn object(State(app): S, UrlPath(hash): UrlPath<String>, h: HeaderMap) -> Response {
    guard!(app, h);
    let dir = app.project.clone();
    blocking(move || {
        let p = Project::open(&dir)?;
        let path = p.object_file(&hash)?;
        if std::fs::metadata(&path)?.len() > 8 << 20 {
            return Err(ExplorerError::Budget("objects over 8 MiB are not served to the browser; use the CLI".into()));
        }
        Ok(serde_json::from_slice(&p.get(&hash)?).unwrap_or_else(|_| json!({"binary": true, "bytes": std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0)})))
    })
    .await
}

/// Trace window: decoded events of a trace manifest between ticks.
async fn trace(State(app): S, Query(q): Query<BTreeMap<String, String>>, h: HeaderMap) -> Response {
    guard!(app, h);
    let dir = app.project.clone();
    blocking(move || {
        let p = Project::open(&dir)?;
        let m = q.get("manifest").ok_or_else(|| ExplorerError::InvalidInput("manifest".into()))?;
        let (man, events) = crate::capture::load(&p, m)?;
        let a: u64 = q.get("from").and_then(|x| x.parse().ok()).unwrap_or(0);
        let b: u64 = q.get("to").and_then(|x| x.parse().ok()).unwrap_or(man.ticks);
        let lim: usize = q.get("limit").and_then(|x| x.parse().ok()).unwrap_or(20000).min(50000);
        let win: Vec<Value> = events.iter().filter(|e| e.tick >= a && e.tick < b).take(lim).map(|e| json!([e.id, e.tick, e.micro, e.neuron, if e.kind == crate::gnat::EventKind::Spike { 0 } else { 1 }])).collect();
        let truncated = events.iter().filter(|e| e.tick >= a && e.tick < b).count() > win.len();
        Ok(json!({"manifest": m, "task": man.task, "seed": man.seed, "ticks": man.ticks, "coverage": man.coverage, "complete": man.complete, "events": win, "truncated_for_display": truncated, "fields": ["id", "tick", "micro", "neuron", "kind"]}))
    })
    .await
}

fn push_event(app: &App, id: &str, ev: Value) {
    if let Ok(mut j) = app.jobs.lock() {
        if let Some(e) = j.get_mut(id) {
            let cursor = e.events.len();
            e.events.push(json!({"cursor": cursor, "event": ev}));
        }
    }
}

async fn submit(State(app): S, h: HeaderMap, axum::Json(body): axum::Json<Value>) -> Response {
    if !mutation_ok(&app, &h) {
        return err(
            StatusCode::FORBIDDEN,
            "session, CSRF header and same origin are required",
        );
    }
    let kind = body["kind"].as_str().unwrap_or("").to_string();
    if ![
        "baseline",
        "analyse",
        "experiment",
        "optimise",
        "validate",
        "export",
    ]
    .contains(&kind.as_str())
    {
        return err(
            StatusCode::BAD_REQUEST,
            "kind: baseline | analyse | experiment | optimise | validate | export",
        );
    }
    // Idempotent submission: identical kind + params map to one job.
    let key = crate::store::sha256_hex(format!("{kind}:{}", body).as_bytes());
    let id = format!("ui-{}", &key[..16]);
    {
        let mut jobs = app.jobs.lock().expect("lock");
        if let Some(j) = jobs.get(&id) {
            if !matches!(j.state.as_str(), "failed" | "cancelled") {
                return axum::Json(json!({"job": id, "state": j.state, "duplicate": true}))
                    .into_response();
            }
        }
        jobs.insert(
            id.clone(),
            JobEntry {
                id: id.clone(),
                kind: kind.clone(),
                params: body.clone(),
                state: "queued".into(),
                result: None,
                error: None,
                cancel: Arc::new(AtomicBool::new(false)),
                events: Vec::new(),
            },
        );
    }
    let n = app.seq.fetch_add(1, Ordering::Relaxed);
    let app2 = app.clone();
    let id2 = id.clone();
    std::thread::spawn(move || {
        let cancel = app2
            .jobs
            .lock()
            .expect("lock")
            .get(&id2)
            .map(|j| j.cancel.clone())
            .expect("job");
        if let Some(j) = app2.jobs.lock().expect("lock").get_mut(&id2) {
            j.state = "running".into();
        }
        push_event(&app2, &id2, json!({"state": "running", "seq": n}));
        let out = (|| -> Result<Value> {
            let cfg = body["config"].as_str().map(PathBuf::from);
            let mut ctx = Ctx::new(Project::open(&app2.project)?, cfg.as_deref())?;
            ctx.cancel = cancel.clone();
            let s = |k: &str| body[k].as_str().map(str::to_string);
            match kind.as_str() {
                "baseline" => pipeline::baseline(
                    &ctx,
                    s("source").as_deref(),
                    &s("split").unwrap_or("development".into()),
                    &s("label").unwrap_or("intact-development".into()),
                    true,
                ),
                "analyse" => {
                    pipeline::analyse(&ctx, &s("run").unwrap_or("intact-development".into()))
                }
                "experiment" => {
                    let plan = body["plan"].clone();
                    let path = ctx.project.tmp_dir().join(format!(
                        "plan-{}.json",
                        &crate::store::sha256_hex(plan.to_string().as_bytes())[..12]
                    ));
                    crate::store::atomic_write(
                        &ctx.project.tmp_dir(),
                        &path,
                        plan.to_string().as_bytes(),
                    )?;
                    pipeline::experiment(&ctx, &path)
                }
                "optimise" => crate::optimise::optimise(
                    &ctx,
                    s("source").as_deref(),
                    &s("select").unwrap_or("min-state-bytes".into()),
                    s("analysis").as_deref().or(Some("intact-development-gnat")),
                ),
                "validate" => pipeline::validate(
                    &ctx,
                    &s("candidate").unwrap_or("selected".into()),
                    &s("split").unwrap_or("locked-validation".into()),
                ),
                _ => {
                    // Named by candidate + profile: a repeated export of the
                    // same candidate returns the existing bundle.
                    let cand = ctx.project.lookup(
                        RecordKind::Candidate,
                        &s("candidate").unwrap_or("selected".into()),
                    )?;
                    let out = ctx.project.root.join("exports").join(format!(
                        "bundle-{}-{}",
                        &cand[..12],
                        &ctx.config_sha[..8]
                    ));
                    if out.join("manifest.json").exists() {
                        let m: Value =
                            serde_json::from_slice(&std::fs::read(out.join("manifest.json"))?)?;
                        let cost: Value =
                            serde_json::from_slice(&std::fs::read(out.join("cost.json"))?)?;
                        return Ok(
                            json!({"bundle": out.display().to_string(), "status": m["status"], "candidate": cand, "cost": cost, "reused": true}),
                        );
                    }
                    pipeline::export(
                        &ctx,
                        &cand,
                        &out,
                        body["exploratory"].as_bool().unwrap_or(false),
                    )
                }
            }
        })();
        let (state, result, error) = match out {
            Ok(v) => ("succeeded", Some(v), None),
            Err(ExplorerError::Cancelled(m)) => ("cancelled", None, Some(m)),
            Err(e) => ("failed", None, Some(e.to_string())),
        };
        if let Some(j) = app2.jobs.lock().expect("lock").get_mut(&id2) {
            j.state = state.into();
            j.result = result;
            j.error = error.clone();
        }
        push_event(&app2, &id2, json!({"state": state, "error": error}));
    });
    axum::Json(json!({"job": id, "state": "queued"})).into_response()
}

fn job_json(j: &JobEntry, full: bool) -> Value {
    json!({"job": j.id, "kind": j.kind, "params": j.params, "state": j.state, "error": j.error, "result": if full { j.result.clone() } else { None }, "events": j.events.len()})
}

async fn jobs(State(app): S, h: HeaderMap) -> Response {
    guard!(app, h);
    let j = app.jobs.lock().expect("lock");
    axum::Json(json!({"jobs": j.values().map(|x| job_json(x, false)).collect::<Vec<_>>()}))
        .into_response()
}

async fn job(State(app): S, UrlPath(id): UrlPath<String>, h: HeaderMap) -> Response {
    guard!(app, h);
    let j = app.jobs.lock().expect("lock");
    match j.get(&id) {
        Some(x) => axum::Json(job_json(x, true)).into_response(),
        None => err(StatusCode::NOT_FOUND, "no such job"),
    }
}

async fn job_events(
    State(app): S,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<BTreeMap<String, String>>,
    h: HeaderMap,
) -> Response {
    guard!(app, h);
    let from: usize = q.get("cursor").and_then(|x| x.parse().ok()).unwrap_or(0);
    let j = app.jobs.lock().expect("lock");
    match j.get(&id) {
        Some(x) => axum::Json(json!({"job": id, "state": x.state, "events": x.events.iter().skip(from).cloned().collect::<Vec<_>>(), "next_cursor": x.events.len()})).into_response(),
        None => err(StatusCode::NOT_FOUND, "no such job"),
    }
}

async fn cancel(State(app): S, UrlPath(id): UrlPath<String>, h: HeaderMap) -> Response {
    if !mutation_ok(&app, &h) {
        return err(
            StatusCode::FORBIDDEN,
            "session, CSRF header and same origin are required",
        );
    }
    let mut j = app.jobs.lock().expect("lock");
    match j.get_mut(&id) {
        Some(x) => {
            x.cancel.store(true, Ordering::Relaxed);
            if x.state == "running" || x.state == "queued" {
                x.state = "pausing".into();
            }
            axum::Json(json!({"job": id, "state": x.state})).into_response()
        }
        None => err(StatusCode::NOT_FOUND, "no such job"),
    }
}

fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/ui/{name}", get(asset))
        .route("/api/explorer/v1/status", get(status))
        .route("/api/explorer/v1/index", get(index_records))
        .route("/api/explorer/v1/sources", get(sources))
        .route("/api/explorer/v1/topology", get(topology))
        .route("/api/explorer/v1/neuron", get(neuron))
        .route("/api/explorer/v1/search", get(search))
        .route("/api/explorer/v1/object/{hash}", get(object))
        .route("/api/explorer/v1/trace", get(trace))
        .route("/api/explorer/v1/jobs", get(jobs).post(submit))
        .route("/api/explorer/v1/jobs/{id}", get(job))
        .route("/api/explorer/v1/jobs/{id}/events", get(job_events))
        .route("/api/explorer/v1/jobs/{id}/cancel", post(cancel))
        .layer(DefaultBodyLimit::max(256 << 10))
        .with_state(app)
}

pub fn serve(project: &Path, listen: &str, open: bool) -> Result<Value> {
    let addr: std::net::SocketAddr = listen
        .parse()
        .map_err(|e| ExplorerError::InvalidInput(format!("--listen {listen}: {e}")))?;
    if !addr.ip().is_loopback() {
        return Err(ExplorerError::InvalidInput(
            "the explorer UI binds loopback only (127.0.0.1 or ::1)".into(),
        ));
    }
    Project::open(project)?;
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(4)
        .enable_all()
        .build()?;
    rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let local = listener.local_addr()?;
        let origin = format!("http://{local}");
        let app = Arc::new(App {
            project: project.to_path_buf(),
            token: random_hex(16)?,
            csrf: random_hex(16)?,
            origin: origin.clone(),
            jobs: Mutex::new(BTreeMap::new()),
            seq: AtomicU64::new(0),
        });
        let url = format!("{origin}/?session={}", app.token);
        eprintln!("aarnn-explorer UI: {url}");
        println!("{}", json!({"listening": local.to_string(), "url": url}));
        if open {
            let _ = std::process::Command::new("xdg-open")
                .arg(&url)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }
        axum::serve(listener, router(app))
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await?;
        Ok::<_, ExplorerError>(())
    })?;
    Ok(json!({"stopped": true}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(dir: &Path) -> Arc<App> {
        Arc::new(App {
            project: dir.to_path_buf(),
            token: "t0k".into(),
            csrf: "c5f".into(),
            origin: "http://127.0.0.1:1".into(),
            jobs: Mutex::new(BTreeMap::new()),
            seq: AtomicU64::new(0),
        })
    }

    #[test]
    fn session_csrf_and_origin_checks() {
        let d = tempfile::tempdir().unwrap();
        let a = app(d.path());
        let mut h = HeaderMap::new();
        assert!(!cookie_ok(&a, &h));
        h.insert(
            header::COOKIE,
            HeaderValue::from_static("x=1; explorer_session=t0k"),
        );
        assert!(cookie_ok(&a, &h));
        assert!(!mutation_ok(&a, &h), "mutations need the CSRF header");
        h.insert("x-explorer-csrf", HeaderValue::from_static("c5f"));
        assert!(mutation_ok(&a, &h));
        h.insert(
            header::ORIGIN,
            HeaderValue::from_static("http://evil.example"),
        );
        assert!(!mutation_ok(&a, &h), "cross-origin mutations are rejected");
    }

    #[test]
    fn non_loopback_listen_is_rejected() {
        let d = tempfile::tempdir().unwrap();
        assert!(serve(d.path(), "0.0.0.0:0", false).is_err());
    }
}
