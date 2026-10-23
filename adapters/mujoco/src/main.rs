use aarnn_mujoco::{
    aer::{Binding, Client, Model},
    world::World,
};
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
#[derive(Deserialize)]
struct Registry {
    schema_version: u32,
    profiles: Vec<Profile>,
}
#[derive(Deserialize)]
struct Profile {
    id: String,
    aliases: Vec<String>,
    model: String,
}
#[derive(Clone)]
struct Options {
    steps: usize,
    realtime: bool,
    output: Option<PathBuf>,
    reset_at: Option<usize>,
    control: aarnn_embodiment::npc::Control,
    headless: bool,
    arm_file: Option<PathBuf>,
    screenshot: Option<PathBuf>,
    json: bool,
    close_view: bool,
}
struct Session {
    world: World,
    start: Instant,
    initial_capture: bool,
}
impl Session {
    fn load(
        path: PathBuf,
        instance: String,
        record: Model,
        binding: Option<Binding>,
        options: &Options,
    ) -> Result<Self, String> {
        let mut world = World::load(&path, record)?;
        world.instance_id = instance;
        world.npc_enabled = options.control.npc(binding.is_some())?;
        if let Some(binding) = binding {
            world.brain_id = Some(binding.brain_id.clone());
            world.client = Some(Client::start(world.model.clone(), binding)?);
        }
        eprintln!(
            "{}",
            serde_json::json!({"status":"ready","pid":std::process::id(),"instance_id":world.instance_id,"profile":world.model.profile_id,"brain_id":world.brain_id,"mode":if world.client.is_some(){"AER1-reference"}else if world.npc_enabled{"programmed-NPC"}else{"silent-mechanics"}})
        );
        if world.model.profile_id == "celegans" && world.client.is_some() {
            eprintln!(
                "C. elegans simulation projection: original reduced mechanics and light/heat/range proxies; calibration differs from Webots. Named muscle response v2, no twitch fallback."
            );
        }
        Ok(Self {
            world,
            start: Instant::now(),
            initial_capture: false,
        })
    }
    /// A private body begins its simulation after its first neural exchange.
    /// Network loading/compilation cannot age out the initial output while
    /// this body's source clock races ahead. Other bodies keep independent
    /// startup state; this is not a neural tick/fleet barrier.
    fn ready(&mut self, options: &Options) -> Result<bool, String> {
        if !gate(options) {
            if let Some(client) = &self.world.client
                && let Some(fault) = client.status().fault
            {
                return Err(format!("reference startup failed: {fault}"));
            }
            return Ok(false);
        }
        let Some(client) = &self.world.client else {
            return Ok(true);
        };
        let status = client.status();
        // Once started, World owns fault/disarm/NPC arbitration. A later
        // channel failure must not freeze other bodies or stop this viewer.
        if status.exchanges > 0 {
            return Ok(true);
        }
        if let Some(fault) = status.fault {
            return Err(format!("reference transport failed: {fault}"));
        }
        if !status.ready {
            return Ok(false);
        }
        if !self.initial_capture {
            let values = self.world.sensors()?;
            self.world
                .client
                .as_ref()
                .unwrap()
                .submit(aarnn_mujoco::aer::Capture {
                    step: self.world.step,
                    timestamp_us: self.world.step * 2000,
                    values,
                });
            self.initial_capture = true;
        }
        Ok(status.exchanges > 0)
    }
    fn advance(&mut self, iteration: usize, options: &Options) -> Result<(), String> {
        if options.reset_at == Some(iteration) {
            self.world.reset();
        }
        self.world.advance()?;
        if self.world.step.is_multiple_of(2500) {
            eprintln!(
                "MuJoCo progress {}: {:.2}s, {} neural exchanges, {} active steps",
                self.world.instance_id,
                self.world.physics.time(),
                self.world
                    .client
                    .as_ref()
                    .map(|c| c.status().exchanges)
                    .unwrap_or(0),
                self.world.active_steps
            );
        }
        if !self.world.physics.qpos().iter().all(|x| x.is_finite()) {
            return Err("nonfinite physical state".into());
        }
        Ok(())
    }
    fn finish(mut self) -> serde_json::Value {
        let status = self.world.client.as_ref().map(Client::status);
        let active_qpos = self.world.physics.qpos().to_vec();
        let snapshot = self.world.physics.snapshot();
        let energy = self.world.measured_energy();
        let (nq, nv, nu) = self.world.physics.dimensions();
        self.world.disarm();
        serde_json::json!({"status":if status.as_ref().is_some_and(|s|s.fault.is_some()){"fail"}else{"pass"},"scope":"native physics and optional reference packets; no learned task/production commitment claim","controller_mode":if self.world.actuation.npc_control{"programmed-NPC"}else if self.world.brain_id.is_some(){"AER1-reference"}else{"silent-mechanics"},"npc_policy":aarnn_embodiment::npc::POLICY,"npc_steps":self.world.npc_steps,"physics":"MuJoCo 3.17.0","instance_id":self.world.instance_id,"profile":self.world.model.profile_id,"brain_id":self.world.brain_id,"model_digest":self.world.model.digest,"io_digest":self.world.model.io.digest,"nq":nq,"nv":nv,"nu":nu,"steps":self.world.step,"reset_epoch":self.world.reset_epoch,"time_s":self.world.physics.time(),"qpos":active_qpos,"energy_J":energy,"elapsed_s":self.start.elapsed().as_secs_f64(),"applied":self.world.applied,"active_steps":self.world.active_steps,"expired":self.world.expired,"transport":status,"physics_snapshot":snapshot})
    }
}
fn gate(options: &Options) -> bool {
    options.arm_file.as_ref().is_none_or(|p| p.is_file())
}
fn simulate(
    path: PathBuf,
    instance: String,
    record: Model,
    binding: Option<Binding>,
    options: Options,
    stop: Arc<AtomicBool>,
) -> Result<serde_json::Value, String> {
    let mut session = Session::load(path, instance, record, binding, &options)?;
    let mut next = Instant::now();
    let mut iteration = 0;
    while iteration < options.steps && !stop.load(Ordering::Relaxed) {
        if !session.ready(&options)? {
            std::thread::sleep(Duration::from_millis(10));
            next = Instant::now();
            continue;
        }
        session.advance(iteration, &options)?;
        iteration += 1;
        if options.realtime {
            next += Duration::from_micros(2000);
            if let Some(delay) = next.checked_duration_since(Instant::now()) {
                std::thread::sleep(delay);
            }
        }
    }
    Ok(session.finish())
}
#[cfg(feature = "viewer")]
fn interactive(
    entries: Vec<(PathBuf, String, Model)>,
    mut bindings: HashMap<String, Binding>,
    options: &Options,
    stop: &AtomicBool,
) -> Result<Vec<serde_json::Value>, String> {
    use aarnn_mujoco::native::viewer::Viewer;
    let mut sessions = Vec::new();
    let mut viewers = Vec::new();
    for (path, instance, model) in entries {
        let binding = bindings.remove(&instance);
        let session = Session::load(path, instance, model, binding, options)?;
        viewers.push(Viewer::new(
            &session.world.physics,
            &session.world.instance_id,
            session.world.model.scene.as_ref().map(|s| s.half_extent_m),
            options.close_view,
        )?);
        sessions.push(session);
    }
    eprintln!(
        "MuJoCo: close window/Esc to stop; Space pause; R reset and inhibit output; F body/habitat view; left drag rotate, right drag or +/- zoom."
    );
    let mut iterations = vec![0usize; sessions.len()];
    let mut next = Instant::now();
    let mut last_frame = next;
    let mut screenshot = false;
    while iterations.iter().any(|n| *n < options.steps) && !stop.load(Ordering::Relaxed) {
        let mut any_running = false;
        for (i, (session, viewer)) in sessions.iter_mut().zip(&mut viewers).enumerate() {
            let (closed, paused, reset) = viewer.poll(&session.world.physics);
            if closed {
                stop.store(true, Ordering::Relaxed);
                break;
            }
            if reset {
                session.world.reset();
            }
            if iterations[i] < options.steps && !paused && session.ready(options)? {
                session.advance(iterations[i], options)?;
                iterations[i] += 1;
                any_running = true;
            }
        }
        let complete = iterations.iter().all(|n| *n >= options.steps);
        if last_frame.elapsed() >= Duration::from_millis(16) || complete {
            for (i, (session, viewer)) in sessions.iter_mut().zip(&mut viewers).enumerate() {
                let title = format!(
                    "AARNN MuJoCo | {} | {} | {:.2}s | exchanges {} | Space pause, R reset",
                    session.world.instance_id,
                    if session.world.brain_id.is_some() {
                        "AER1 reference"
                    } else if session.world.npc_enabled {
                        "NPC"
                    } else {
                        "silent"
                    },
                    session.world.physics.time(),
                    session
                        .world
                        .client
                        .as_ref()
                        .map(|c| c.status().exchanges)
                        .unwrap_or(0)
                );
                viewer.render(
                    &mut session.world.physics,
                    &title,
                    if i == 0 && complete {
                        screenshot = true;
                        options.screenshot.as_deref()
                    } else {
                        None
                    },
                )?;
            }
            last_frame = Instant::now();
        }
        if options.realtime || !any_running {
            next += Duration::from_micros(2000);
            if let Some(delay) = next.checked_duration_since(Instant::now()) {
                std::thread::sleep(delay);
            } else {
                next = Instant::now();
            }
        }
    }
    if !screenshot && let Some(path) = &options.screenshot {
        viewers[0].render(&mut sessions[0].world.physics, "AARNN MuJoCo", Some(path))?;
    }
    // GL objects/windows are dropped on this thread before their physics owners.
    drop(viewers);
    Ok(sessions.into_iter().map(Session::finish).collect())
}
fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "--help") {
        println!(
            "aarnn-mujoco MODEL.xml | --robots PROFILE=COUNT [--control auto|npc|silent] [--assets DIR] [--steps N] [--realtime] [--reference-binding JSON | --reference-bindings MAP.json] [--output JSON] [--reset-at STEP]\nNative Rust interactive viewer by default; --headless uses a bounded diagnostic run. --steps N bounds either mode; --json prints full state; --output retains evidence. --arm-file FILE waits for managed startup. --screenshot FILE writes a rendered PPM. No production effect authority."
        );
        return Ok(());
    }
    let mut options = Options {
        steps: usize::MAX,
        realtime: true,
        output: None,
        reset_at: None,
        control: aarnn_embodiment::npc::Control::Auto,
        headless: false,
        arm_file: None,
        screenshot: None,
        json: false,
        close_view: false,
    };
    let mut model = None;
    let mut robots = None;
    let mut assets = PathBuf::from("sim/content/embodiments/generated");
    let mut single_binding = None;
    let mut bindings_path = None;
    let mut realtime_explicit = false;
    let mut i = 0;
    while i < args.len() {
        let value = |i: usize| args.get(i + 1).ok_or("option value missing");
        match args[i].as_str() {
            "--control" => {
                options.control = aarnn_embodiment::npc::Control::parse(value(i)?)?;
                i += 1;
            }
            "--robots" => {
                robots = Some(value(i)?.clone());
                i += 1;
            }
            "--assets" => {
                assets = PathBuf::from(value(i)?);
                i += 1;
            }
            "--steps" => {
                options.steps = value(i)?.parse().map_err(|_| "invalid steps")?;
                if !(1..=10_000_000).contains(&options.steps) {
                    return Err("step budget".into());
                }
                i += 1;
            }
            "--headless" => {
                options.headless = true;
            }
            "--json" => options.json = true,
            "--view" => {
                options.close_view = match value(i)?.as_str() {
                    "body" => true,
                    "habitat" => false,
                    _ => return Err("--view expects body or habitat".into()),
                };
                i += 1;
            }
            "--arm-file" => {
                options.arm_file = Some(PathBuf::from(value(i)?));
                i += 1;
            }
            "--screenshot" => {
                options.screenshot = Some(PathBuf::from(value(i)?));
                i += 1;
            }
            "--realtime" => realtime_explicit = true,
            "--reference-binding" => {
                single_binding = Some(PathBuf::from(value(i)?));
                i += 1;
            }
            "--reference-bindings" => {
                bindings_path = Some(PathBuf::from(value(i)?));
                i += 1;
            }
            "--output" => {
                options.output = Some(PathBuf::from(value(i)?));
                i += 1;
            }
            "--reset-at" => {
                options.reset_at = Some(
                    value(i)?
                        .parse::<usize>()
                        .map_err(|_| "invalid reset step")?,
                );
                i += 1;
            }
            other if !other.starts_with('-') && model.is_none() => {
                model = Some(PathBuf::from(other))
            }
            other => return Err(format!("unsupported native option {other}")),
        }
        i += 1;
    }
    if options.headless && options.steps == usize::MAX {
        options.steps = 1000;
    }
    options.realtime = !options.headless || realtime_explicit;
    if options.headless && options.screenshot.is_some() {
        return Err("screenshot requires a viewer".into());
    }
    if options.steps != usize::MAX && !(1..=10_000_000).contains(&options.steps) {
        return Err("step budget".into());
    }
    if model.is_some() == robots.is_some() {
        return Err("select one MODEL.xml or --robots spec".into());
    }
    let mut entries = Vec::new();
    if let Some(path) = model {
        let record = aarnn_mujoco::schema::load_model(&path)?;
        entries.push((path, format!("{}_0", record.profile_id), record));
    } else if let Some(spec) = robots {
        let registry: Registry = serde_json::from_slice(
            &std::fs::read(assets.join("registry.generated.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        if registry.schema_version != 1 {
            return Err("registry schema".into());
        }
        let mut indices: HashMap<String, usize> = HashMap::new();
        for token in spec.split([',', ';']) {
            let (name, count) = token.split_once('=').ok_or("expected profile=count")?;
            let normalized = name.trim().to_ascii_lowercase().replace(['-', ' '], "_");
            let profile = registry
                .profiles
                .iter()
                .find(|p| p.id == normalized || p.aliases.contains(&normalized))
                .ok_or_else(|| format!("unknown profile {name}"))?;
            let count: usize = count.parse().map_err(|_| "invalid count")?;
            if count > 32 || entries.len() + count > 32 {
                return Err("at most 32 independent worlds".into());
            }
            for _ in 0..count {
                let index = indices.entry(profile.id.clone()).or_default();
                let instance = format!("{}_{index}", profile.id);
                *index += 1;
                let path = assets.join(&profile.model);
                let record = aarnn_mujoco::schema::load_model(&path)?;
                if record.profile_id != profile.id {
                    return Err("registry/model identity mismatch".into());
                }
                entries.push((path, instance, record));
            }
        }
    }
    if entries.is_empty() {
        return Err("population resolves to zero bodies".into());
    }
    let mut bindings: HashMap<String, Binding> = if let Some(path) = bindings_path {
        serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?
    } else {
        HashMap::new()
    };
    if let Some(path) = single_binding {
        if entries.len() != 1 || !bindings.is_empty() {
            return Err("single binding requires one body".into());
        }
        bindings.insert(
            entries[0].1.clone(),
            serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?,
        );
    }
    let mut brains = HashSet::new();
    let mut endpoints = HashSet::new();
    for (_, instance, record) in &entries {
        if let Some(binding) = bindings.get(instance) {
            binding.validate(record)?;
            if !brains.insert(binding.brain_id.clone())
                || !endpoints.insert((binding.host.clone(), binding.port))
            {
                return Err(
                    "independent bodies require distinct brain and endpoint identities".into(),
                );
            }
        }
    }
    if bindings
        .keys()
        .any(|id| !entries.iter().any(|e| &e.1 == id))
    {
        return Err("binding for absent body".into());
    }
    let stop = Arc::new(AtomicBool::new(false));
    for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
        signal_hook::flag::register(signal, stop.clone()).map_err(|e| e.to_string())?;
    }
    let reports = if options.headless {
        let mut workers = Vec::new();
        for (path, instance, record) in entries {
            let binding = bindings.remove(&instance);
            let config = options.clone();
            let cancel = stop.clone();
            workers.push(std::thread::spawn(move || {
                simulate(path, instance, record, binding, config, cancel)
            }));
        }
        let mut reports = Vec::new();
        for worker in workers {
            reports.push(worker.join().map_err(|_| "physics owner panicked")??);
        }
        reports
    } else {
        #[cfg(feature = "viewer")]
        {
            interactive(entries, bindings, &options, &stop)?
        }
        #[cfg(not(feature = "viewer"))]
        {
            return Err("interactive support requires --features viewer; use --headless --steps N for diagnostics".into());
        }
    };
    let failed = reports.iter().any(|r| r["status"] != "pass");
    let result = serde_json::json!({"schema_version":1,"independent_worlds":true,"worlds":reports});
    if let Some(path) = &options.output {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, serde_json::to_vec_pretty(&result).unwrap())
            .map_err(|e| e.to_string())?;
    }
    if options.json {
        println!("{result}");
    } else {
        for row in result["worlds"].as_array().unwrap() {
            println!(
                "MuJoCo {}: {} steps, {:.3}s simulated, {} neural exchanges, {} output spikes, {} active steps ({})",
                row["instance_id"].as_str().unwrap_or("body"),
                row["steps"],
                row["time_s"].as_f64().unwrap_or(0.),
                row["transport"]["exchanges"].as_u64().unwrap_or(0),
                row["transport"]["output_spikes"].as_u64().unwrap_or(0),
                row["active_steps"],
                row["controller_mode"].as_str().unwrap_or("unknown")
            );
        }
        if let Some(path) = &options.output {
            println!("Evidence: {}", path.display());
        }
    }
    if failed {
        return Err("reference transport failed".into());
    }
    Ok(())
}
