//! Private, loopback-only simulation authority. The physics thread alone owns
//! MuJoCo and AER. HTTP/viewer requests never supply neural sensory samples.
use aarnn_mujoco::{
    aer::{Binding, Client},
    schema,
    world::{Checkpoint, World},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    time::{Duration, Instant},
};
use tokio::sync::{Semaphore, oneshot};

struct Envelope {
    command: Command,
    reply: oneshot::Sender<Result<Value, String>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Command {
    action: String,
    reset_epoch: u64,
    model_digest: String,
    slot: Option<usize>,
    position: Option<[f64; 3]>,
    checkpoint: Option<Checkpoint>,
    voxels: Option<Vec<[f64; 3]>>,
    world_config_epoch: Option<u64>,
}
#[derive(Clone)]
struct Service {
    token_hash: [u8; 32],
    model: Arc<Value>,
    snapshot: Arc<Mutex<Value>>,
    commands: SyncSender<Envelope>,
    emergency: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    slots: Arc<Semaphore>,
}
fn error(status: StatusCode, reason: impl Into<String>) -> Response {
    (status, Json(json!({"error":reason.into()}))).into_response()
}
fn authorised(headers: &HeaderMap, service: &Service) -> bool {
    if let Some(origin) = headers.get("origin") {
        let Some(host) = headers.get("host").and_then(|s| s.to_str().ok()) else {
            return false;
        };
        if origin.to_str().ok() != Some(format!("http://{host}").as_str()) {
            return false;
        }
    }
    let Some(value) = headers
        .get("authorization")
        .and_then(|s| s.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
    else {
        return false;
    };
    let hash: [u8; 32] = Sha256::digest(value.as_bytes()).into();
    hash.iter()
        .zip(service.token_hash)
        .fold(0u8, |a, (x, y)| a | (x ^ y))
        == 0
}
async fn state(State(s): State<Service>, headers: HeaderMap) -> Response {
    if !authorised(&headers, &s) {
        return error(StatusCode::FORBIDDEN, "private simulation token required");
    }
    let value = s.snapshot.lock().unwrap().clone();
    if value["status"] == "fail" {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            value["error"].as_str().unwrap_or("physics owner failed"),
        );
    }
    Json(value).into_response()
}
async fn model(State(s): State<Service>, headers: HeaderMap) -> Response {
    if !authorised(&headers, &s) {
        return error(StatusCode::FORBIDDEN, "private simulation token required");
    }
    Json((*s.model).clone()).into_response()
}
async fn stop(State(s): State<Service>, headers: HeaderMap) -> Response {
    if !authorised(&headers, &s) {
        return error(StatusCode::FORBIDDEN, "private simulation token required");
    }
    s.emergency.store(true, Ordering::Release);
    (
        StatusCode::ACCEPTED,
        Json(json!({"status":"disarm-requested"})),
    )
        .into_response()
}
async fn command(
    State(s): State<Service>,
    headers: HeaderMap,
    Json(command): Json<Command>,
) -> Response {
    if !authorised(&headers, &s) {
        return error(StatusCode::FORBIDDEN, "private simulation token required");
    }
    let Ok(_permit) = s.slots.try_acquire() else {
        return error(StatusCode::TOO_MANY_REQUESTS, "command budget exhausted");
    };
    let (tx, rx) = oneshot::channel();
    if s.commands
        .try_send(Envelope { command, reply: tx })
        .is_err()
    {
        return error(StatusCode::TOO_MANY_REQUESTS, "command queue full");
    }
    match tokio::time::timeout(Duration::from_secs(1), rx).await {
        Ok(Ok(Ok(value))) => Json(value).into_response(),
        Ok(Ok(Err(reason))) => error(StatusCode::CONFLICT, reason),
        _ => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "outcome unknown; read state before retry",
        ),
    }
}
fn private_token(path: &Path) -> Result<String, String> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let mut bytes = [0; 32];
    if !path.exists() {
        std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut bytes))
            .map_err(|e| e.to_string())?;
        let token = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| e.to_string())?;
        writeln!(file, "{token}")
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
    }
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.permissions().mode() & 0o077 != 0 || meta.len() > 4096 {
        return Err("token file must be a private regular file (0600)".into());
    }
    let token = fs::read_to_string(path)
        .map_err(|e| e.to_string())?
        .trim()
        .to_owned();
    if token.len() < 32 {
        return Err("token must contain at least 32 characters".into());
    }
    Ok(token)
}
fn mirror_model(path: &Path) -> Result<PathBuf, String> {
    let source = fs::read_to_string(path).map_err(|e| e.to_string())?;
    if source.matches("</worldbody>").count() != 1 {
        return Err("one native worldbody required".into());
    }
    let mut voxels = String::new();
    for slot in 0..32 {
        voxels.push_str(&format!(r#"<body name="world_voxel_{slot}" mocap="true" pos="{} 100 100"><geom name="world_voxel_{slot}_collision" type="box" size=".5 .5 .5" rgba=".45 .35 .25 1"/></body>"#,100+slot));
    }
    let augmented = source.replace("</worldbody>", &format!("{voxels}</worldbody>"));
    // A sibling retains all relative licensed asset references. Never overwrite
    // the pinned robot model. create_new refuses collisions/symlinks.
    let temp = path.with_file_name(format!(".physics-mirror-{}.xml", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    if let Err(e) = file.write_all(augmented.as_bytes()) {
        let _ = fs::remove_file(&temp);
        return Err(e.to_string());
    }
    Ok(temp)
}
fn physics_owner(
    path: PathBuf,
    binding: Option<Binding>,
    control: aarnn_embodiment::npc::Control,
    service: Service,
    rx: mpsc::Receiver<Envelope>,
    ready: SyncSender<Result<(), String>>,
) {
    let run = || -> Result<(), String> {
        let record = schema::load_model(&path)?;
        let temporary = mirror_model(&path)?;
        let loaded = World::load(&temporary, record);
        let _ = fs::remove_file(temporary);
        let mut world = loaded?;
        world.npc_enabled = control.npc(binding.is_some())?;
        if let Some(binding) = binding {
            world.brain_id = Some(binding.brain_id.clone());
            world.client = Some(Client::start(world.model.clone(), binding)?);
        }
        let mut paused = false;
        let mut publication = 0u64;
        let mut epoch = 0u64;
        let mut obstacles = BTreeMap::<usize, [f64; 3]>::new();
        let mut deadline = Instant::now();
        let _ = ready.send(Ok(()));
        while !service.stop.load(Ordering::Acquire) {
            if service.emergency.swap(false, Ordering::AcqRel) {
                world.stop();
            }
            for envelope in rx.try_iter().take(8) {
                // A cancelled HTTP command has no effect when it has not yet
                // started. A timeout after application is reported as ambiguous.
                if envelope.reply.is_closed() {
                    continue;
                }
                let c = envelope.command;
                let result = (|| -> Result<Value, String> {
                    if c.model_digest != world.model.digest || c.reset_epoch != world.reset_epoch {
                        return Err("stale model/reset epoch".into());
                    }
                    match c.action.as_str() {
                        "pause" => paused = true,
                        "resume" => paused = false,
                        "step" => {
                            if !paused {
                                return Err("single step requires pause".into());
                            }
                            world.advance()?;
                        }
                        "reset" => {
                            world.reset();
                            obstacles.clear();
                            epoch += 1;
                        }
                        "arm" => world.arm()?,
                        "obstacle" => {
                            let slot = c.slot.ok_or("voxel slot required")?;
                            let point = c.position.ok_or("voxel position required")?;
                            world.physics.place_voxel(slot, point)?;
                            obstacles.insert(slot, point);
                            epoch += 1;
                        }
                        "collision_set" => {
                            if !paused || c.world_config_epoch != Some(epoch) {
                                return Err("pause and current environment epoch required".into());
                            }
                            let positions = c.voxels.ok_or("bounded voxel scene required")?;
                            world.physics.replace_voxels(&positions)?;
                            obstacles = positions.into_iter().enumerate().collect();
                            epoch = epoch.checked_add(1).ok_or("environment epoch overflow")?;
                        }
                        "checkpoint" => {
                            return Ok(
                                json!({"status":"captured","checkpoint":world.checkpoint(),"scope":"local embodiment only; no remote brain or in-flight exchange"}),
                            );
                        }
                        "measure" => {
                            if world.client.is_some() {
                                return Err(
                                    "diagnostic measurements require an unbound world".into()
                                );
                            }
                            return Ok(
                                json!({"status":"measured","source_step":world.step,"values":world.sensors()?}),
                            );
                        }
                        "restore" => {
                            world.restore(&c.checkpoint.ok_or("checkpoint required")?)?;
                            obstacles.clear();
                            for pose in world.physics.poses() {
                                if let Some(slot) = pose["name"]
                                    .as_str()
                                    .and_then(|n| n.strip_prefix("world_voxel_"))
                                    .and_then(|n| n.parse::<usize>().ok())
                                {
                                    let point: [f64; 3] =
                                        serde_json::from_value(pose["position"].clone())
                                            .map_err(|e| e.to_string())?;
                                    if point.iter().all(|x| x.abs() <= 10.) {
                                        obstacles.insert(slot, point);
                                    }
                                }
                            }
                            epoch += 1;
                        }
                        _ => return Err("unsupported command".into()),
                    }
                    Ok(
                        json!({"status":"applied","reset_epoch":world.reset_epoch,"world_config_epoch":epoch}),
                    )
                })();
                let _ = envelope.reply.send(result);
            }
            if !paused {
                world.advance()?;
            }
            publication += 1;
            let status = world.client.as_ref().map(Client::status);
            *service.snapshot.lock().unwrap() = json!({"schema_version":1,"world_id":world.instance_id,"profile_id":world.model.profile_id,"model_digest":world.model.digest,"io_digest":world.model.io.digest,"brain_id":world.brain_id,"reset_epoch":world.reset_epoch,"step":world.step,"time_us":world.step*2000,"npc_policy":aarnn_embodiment::npc::POLICY,"npc_steps":world.npc_steps,"controller_mode":if world.actuation.npc_control{"programmed-NPC"}else if world.client.is_some(){"AER1-reference"}else{"silent-mechanics"},"disarmed":world.disarmed,"actuation_inhibited":world.actuation_inhibited,"energy_J":world.measured_energy(),"publication":publication,"paused":paused,"world_config_epoch":epoch,"obstacles":obstacles,"poses":world.physics.poses(),"transport":status,"viewer_mode":"functional-physics-service","authority":"MuJoCo-3.17.0-Rust-reference","environment_schema":"voxel-mirrors-v1-32-slots","quality":if world.actuation.npc_control{"programmed-NPC"}else if world.client.is_some(){"reference-uncommitted-neural-effects"}else{"silent-mechanics"}});
            deadline += Duration::from_millis(2);
            if let Some(wait) = deadline.checked_duration_since(Instant::now()) {
                std::thread::sleep(wait);
            } else {
                deadline = Instant::now();
            }
        }
        world.stop();
        world.client.take();
        Ok(())
    };
    if let Err(reason) = run() {
        let _ = ready.try_send(Err(reason.clone()));
        *service.snapshot.lock().unwrap() = json!({"status":"fail","error":reason});
        service.stop.store(true, Ordering::Release);
    }
}
#[tokio::main]
async fn main() -> Result<(), String> {
    let mut control = aarnn_embodiment::npc::Control::Auto;
    let mut profile = "crow_corvus_corone".to_owned();
    let mut port = 8092u16;
    let mut token_file = None;
    let mut binding_path = None;
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.iter().any(|s| s == "--help") {
        println!(
            "aarnn-physics-service --profile ID --token-file PRIVATE_PATH [--port 8092] [--reference-binding JSON]\nLoopback simulation only. One physics owner; viewers never produce AER. No production effect authority."
        );
        return Ok(());
    }
    for pair in args.chunks(2) {
        if pair.len() != 2 {
            return Err("option value required".into());
        }
        match pair[0].as_str() {
            "--control" => control = aarnn_embodiment::npc::Control::parse(&pair[1])?,
            "--profile" => profile = pair[1].clone(),
            "--port" => port = pair[1].parse().map_err(|_| "invalid port")?,
            "--token-file" => token_file = Some(PathBuf::from(&pair[1])),
            "--reference-binding" => binding_path = Some(PathBuf::from(&pair[1])),
            _ => return Err("unsupported service option".into()),
        }
    }
    if profile.is_empty()
        || !profile
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err("canonical profile ID required".into());
    }
    let token_file = token_file.ok_or("--token-file required")?;
    let token = private_token(&token_file)?;
    let path = PathBuf::from("sim/content/embodiments/generated").join(format!("{profile}.xml"));
    let record = schema::load_model(&path)?;
    let binding = if let Some(path) = binding_path {
        let b: Binding = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        b.validate(&record)?;
        Some(b)
    } else {
        None
    };
    let model_record =
        serde_json::from_slice(&fs::read(path.with_extension("json")).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let (tx, rx) = mpsc::sync_channel(32);
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let service = Service {
        token_hash: Sha256::digest(token.as_bytes()).into(),
        model: Arc::new(model_record),
        snapshot: Arc::new(Mutex::new(json!({"status":"starting"}))),
        commands: tx,
        emergency: Arc::new(AtomicBool::new(false)),
        stop: Arc::new(AtomicBool::new(false)),
        slots: Arc::new(Semaphore::new(16)),
    };
    for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
        signal_hook::flag::register(signal, service.stop.clone()).map_err(|e| e.to_string())?;
    }
    let worker_service = service.clone();
    let worker = std::thread::spawn(move || {
        physics_owner(path, binding, control, worker_service, rx, ready_tx)
    });
    ready_rx
        .recv_timeout(Duration::from_secs(30))
        .map_err(|e| e.to_string())??;
    let app = Router::new()
        .route(
            "/",
            get(|| async { Html(include_str!("../../../web_ui/physics-view.html")) }),
        )
        .route(
            "/viewer.js",
            get(|| async {
                (
                    [("content-type", "text/javascript")],
                    include_str!("../../../web_ui/physics-view.js"),
                )
            }),
        )
        .route(
            "/webgl-world.js",
            get(|| async {
                (
                    [("content-type", "text/javascript")],
                    include_str!("../../../web_ui/webgl-world.js"),
                )
            }),
        )
        .route("/state", get(state))
        .route("/model", get(model))
        .route("/command", post(command))
        .route("/stop", post(stop))
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024))
        .with_state(service.clone());
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .map_err(|e| e.to_string())?;
    println!(
        "{}",
        json!({"status":"ready","url":format!("http://{}/",listener.local_addr().unwrap()),"token_file":token_file,"profile":profile})
    );
    let shutdown = service.stop.clone();
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            while !shutdown.load(Ordering::Acquire) {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await;
    service.stop.store(true, Ordering::Release);
    worker.join().map_err(|_| "physics owner panicked")?;
    let state = service.snapshot.lock().unwrap();
    if state["status"] == "fail" {
        return Err(state["error"]
            .as_str()
            .unwrap_or("physics owner failed")
            .to_owned());
    }
    result.map_err(|e| e.to_string())
}
