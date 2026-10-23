//! Bounded numerical/AER ABI for native simulation engines, not management or
//! a production peripheral authority. Buffers are caller-owned only for a call;
//! handles are registry IDs rather than Rust pointers. No panic crosses C.
use aarnn_embodiment::{
    actuation::Actuation,
    aer::{Binding, Capture, Client, Model},
    force,
    projection::Projection,
    schema,
};
use std::{
    cell::RefCell,
    collections::HashMap,
    path::Path,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};
pub const NM_EMBODIMENT_ABI_VERSION: u32 = 1;
pub const NM_PANEL_INPUT_WIDTH: u32 = 13;
pub const NM_MAX_CHANNELS: u32 = 4096;
pub const NM_CONTROL_AUTO: u32 = 0;
pub const NM_CONTROL_NPC: u32 = 1;
pub const NM_CONTROL_SILENT: u32 = 2;
pub const NM_CONTROL_AARNN: u32 = 3;
thread_local! {static ERROR:RefCell<String>=const{RefCell::new(String::new())};}
struct Session {
    model: Model,
    binding: Option<Binding>,
    client: Option<Client>,
    actuation: Actuation,
    projection: Projection,
    last_step: u64,
    reset_epoch: u64,
    closed: bool,
    npc_enabled: bool,
}
static SESSIONS: OnceLock<Mutex<HashMap<u64, Arc<Mutex<Session>>>>> = OnceLock::new();
static NEXT: AtomicU64 = AtomicU64::new(1);
fn registry() -> &'static Mutex<HashMap<u64, Arc<Mutex<Session>>>> {
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}
fn session(handle: u64) -> Result<Arc<Mutex<Session>>, String> {
    registry()
        .lock()
        .map_err(|_| "registry unavailable")?
        .get(&handle)
        .cloned()
        .ok_or("unknown/closed simulation handle".into())
}
fn guarded(f: impl FnOnce() -> Result<i32, String>) -> i32 {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            ERROR.with(|s| *s.borrow_mut() = error);
            -1
        }
        Err(_) => {
            ERROR.with(|s| {
                *s.borrow_mut() =
                    "native simulation ABI panicked; disarm and recreate the session".into()
            });
            -2
        }
    }
}
// SAFETY: Foreign callers must provide live nonaliasing buffers with at least
// the documented element counts. Lengths, nulls and scalar ranges are checked
// before native reads. Foreign allocation validity is the caller's contract.
unsafe fn bytes<'a>(ptr: *const u8, len: u32, max: u32) -> Result<&'a [u8], String> {
    if len > max || ptr.is_null() {
        return Err("invalid bounded UTF-8 buffer".into());
    }
    Ok(unsafe { std::slice::from_raw_parts(ptr, len as usize) })
}
unsafe fn text(ptr: *const u8, len: u32, max: u32) -> Result<String, String> {
    String::from_utf8(unsafe { bytes(ptr, len, max)? }.to_vec()).map_err(|_| "invalid UTF-8".into())
}
fn read_json(path: &Path, max: u64) -> Result<serde_json::Value, String> {
    if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > max {
        return Err("JSON source budget".into());
    }
    serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
/// Return the ABI version before invoking any other exported operation.
#[unsafe(no_mangle)]
pub extern "C" fn nm_embodiment_version() -> u32 {
    NM_EMBODIMENT_ABI_VERSION
}
/// Copy the current thread's last error into a caller-owned bounded byte buffer.
/// Returns byte length (no NUL terminator), or a negative error code.
///
/// # Safety
/// Every non-null pointer must reference a live, aligned allocation of at least
/// its declared element count for this call. Input and output allocations must
/// not overlap; no other thread may mutate them during the call. Handles are
/// integer IDs and are validated; they are never dereferenced as pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nm_embodiment_error(output: *mut u8, capacity: u32) -> i32 {
    guarded(|| {
        if output.is_null() || capacity > 4096 {
            return Err("invalid error buffer".into());
        }
        let value = ERROR.with(|s| s.borrow().clone());
        let count = value.len().min(capacity as usize);
        unsafe { std::ptr::copy_nonoverlapping(value.as_ptr(), output, count) };
        Ok(count as i32)
    })
}
/// Verify a generated rigid graph against its canonical model/IO and source file.
///
/// # Safety
/// Every non-null pointer must reference a live, aligned allocation of at least
/// its declared element count for this call. Input and output allocations must
/// not overlap; no other thread may mutate them during the call. Handles are
/// integer IDs and are validated; they are never dereferenced as pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nm_embodiment_verify_graph(
    model_path: *const u8,
    model_len: u32,
    graph_path: *const u8,
    graph_len: u32,
) -> i32 {
    guarded(|| {
        let model = unsafe { text(model_path, model_len, 16384)? };
        let graph = unsafe { text(graph_path, graph_len, 16384)? };
        let record = schema::load_model(Path::new(&model))?;
        let graph = read_json(Path::new(&graph), 64 * 1024 * 1024)?;
        schema::verify_record(&graph)?;
        if graph["model_digest"] != record.digest
            || graph["io_digest"] != record.io.digest
            || graph["compiler"] != "embodiment-rigid-graph-v2"
        {
            return Err("rigid graph/model binding mismatch".into());
        }
        Ok(0)
    })
}
/// Open a silent simulation session when binding_len is zero; otherwise open a
/// strict loopback AER1 reference binding. No production authority is conferred.
///
/// # Safety
/// Every non-null pointer must reference a live, aligned allocation of at least
/// its declared element count for this call. Input and output allocations must
/// not overlap; no other thread may mutate them during the call. Handles are
/// integer IDs and are validated; they are never dereferenced as pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nm_reference_open(
    model_path: *const u8,
    model_len: u32,
    binding_json: *const u8,
    binding_len: u32,
    output_handle: *mut u64,
) -> i32 {
    guarded(|| {
        if output_handle.is_null() {
            return Err("output handle buffer required".into());
        }
        unsafe {
            *output_handle = 0;
        }
        let path = unsafe { text(model_path, model_len, 16384)? };
        let model = schema::load_model(Path::new(&path))?;
        let binding: Option<Binding> = if binding_len == 0 {
            None
        } else {
            Some(
                serde_json::from_slice(unsafe { bytes(binding_json, binding_len, 65536)? })
                    .map_err(|e| e.to_string())?,
            )
        };
        let client = if let Some(b) = binding.as_ref() {
            Some(Client::start(model.clone(), b.clone())?)
        } else {
            None
        };
        let state = Session {
            actuation: Actuation::new(&model),
            projection: Projection::default(),
            model,
            binding,
            client,
            last_step: 0,
            reset_epoch: 0,
            closed: false,
            npc_enabled: false,
        };
        let id = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(1))
            .map_err(|_| "simulation handle space exhausted")?;
        registry()
            .lock()
            .map_err(|_| "registry unavailable")?
            .insert(id, Arc::new(Mutex::new(state)));
        unsafe {
            *output_handle = id;
        }
        Ok(0)
    })
}
/// Close once; subsequent uses fail. Joining this session's bounded worker does
/// not hold the registry or another instance's state lock.
///
/// # Safety
/// Every non-null pointer must reference a live, aligned allocation of at least
/// its declared element count for this call. Input and output allocations must
/// not overlap; no other thread may mutate them during the call. Handles are
/// integer IDs and are validated; they are never dereferenced as pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nm_reference_close(handle: u64) -> i32 {
    guarded(|| {
        let state = registry()
            .lock()
            .map_err(|_| "registry unavailable")?
            .remove(&handle)
            .ok_or("already closed simulation handle")?;
        let client = {
            let mut s = state.lock().map_err(|_| "session unavailable")?;
            s.closed = true;
            s.actuation.stop();
            s.client.take()
        };
        drop(client);
        Ok(0)
    })
}
/// Latch disarm. Re-arming requires a new explicit binding/session; reset never
/// calls a remote brain-management operation.
///
/// # Safety
/// Every non-null pointer must reference a live, aligned allocation of at least
/// its declared element count for this call. Input and output allocations must
/// not overlap; no other thread may mutate them during the call. Handles are
/// integer IDs and are validated; they are never dereferenced as pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nm_reference_disarm(handle: u64) -> i32 {
    guarded(|| {
        let state = session(handle)?;
        let mut state = state.lock().map_err(|_| "session unavailable")?;
        state.actuation.stop();
        Ok(0)
    })
}
/// Reset this body's source clock/projection at an owner-controlled physics
/// boundary. Disconnect and discard the old reference stream, advance the
/// local epoch and latch stop. The remote brain is never reset or reconnected.
/// Re-arming requires a new explicit session. Physics is reset by its engine.
///
/// # Safety
/// Integer handles are validated and never dereferenced as pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nm_reference_reset(handle: u64) -> i32 {
    guarded(|| {
        let state = session(handle)?;
        let client = {
            let mut s = state.lock().map_err(|_| "session unavailable")?;
            if s.closed {
                return Err("closed session".into());
            }
            s.actuation.reset();
            s.actuation.npc_control = false;
            s.reset_epoch = s
                .reset_epoch
                .checked_add(1)
                .ok_or("reset epoch exhausted")?;
            s.last_step = 0;
            s.projection = Projection::default();
            s.binding = None;
            s.client.take()
        };
        // Joining network shutdown never holds an instance/registry lock.
        drop(client);
        Ok(0)
    })
}
/// One fixed 2ms source step. sensor_count must match the verified model. Output
/// is one bounded native control target per actuator plus syrinx/respiration
/// (two zeros for non-crow). Returns 1 for active reference output, 0 for silent
/// or inhibited, negative on failure. No socket I/O blocks this call.
///
/// # Safety
/// Every non-null pointer must reference a live, aligned allocation of at least
/// its declared element count for this call. Input and output allocations must
/// not overlap; no other thread may mutate them during the call. Handles are
/// integer IDs and are validated; they are never dereferenced as pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nm_reference_step(
    handle: u64,
    source_step: u64,
    sensors: *const f64,
    sensor_count: u32,
    controls: *mut f64,
    control_capacity: u32,
) -> i32 {
    guarded(|| {
        // Clear only a valid bounded output allocation. Any validation failure
        // latches disarm on an existing session, including a bad clock or width.
        if !controls.is_null() && control_capacity <= NM_MAX_CHANNELS {
            unsafe { std::ptr::write_bytes(controls, 0, control_capacity as usize) };
        }
        let state = session(handle)?;
        let mut s = state.lock().map_err(|_| "session unavailable")?;
        let result = (|| -> Result<i32, String> {
            if s.closed
                || sensors.is_null()
                || controls.is_null()
                || sensor_count > NM_MAX_CHANNELS
                || control_capacity > NM_MAX_CHANNELS
            {
                return Err("invalid session/bounded numerical buffer".into());
            }
            if sensor_count as usize != s.model.io.sensors.len()
                || control_capacity as usize != s.model.actuators.len() + 2
                || source_step <= s.last_step
                || source_step.checked_mul(2000).is_none()
            {
                return Err("source clock or mapping contract mismatch".into());
            }
            let values = unsafe { std::slice::from_raw_parts(sensors, sensor_count as usize) };
            if !values
                .iter()
                .all(|v| v.is_finite() && (0. ..=1.).contains(v))
            {
                return Err("invalid normalized measurement".into());
            }
            let neural_ready = s.client.as_ref().is_some_and(|c| {
                let status = c.status();
                status.ready && status.fault.is_none()
            });
            let reply = s.client.as_ref().and_then(Client::receive);
            let Session {
                model,
                actuation,
                npc_enabled,
                ..
            } = &mut *s;
            actuation.select(
                model,
                source_step,
                *npc_enabled,
                neural_ready,
                reply.as_ref(),
                Some(values),
            )?;
            let output = s.actuation.controls(&s.model);
            unsafe {
                std::ptr::copy_nonoverlapping(output.as_ptr(), controls, output.len());
            }
            s.last_step = source_step;
            if s.client.as_ref().is_some_and(|c| c.status().ready) {
                s.client.as_ref().unwrap().submit(Capture {
                    step: source_step,
                    timestamp_us: source_step * 2000,
                    values: values.to_vec(),
                });
            }
            Ok(if s.actuation.disarmed {
                0
            } else if s.actuation.npc_control {
                2
            } else {
                1
            })
        })();
        if result.is_err() {
            s.actuation.stop();
        }
        result
    })
}
/// Copy a versioned JSON status record for evidence. Returns byte length, no
/// NUL. Capacity is bounded; undersized buffers fail without partial JSON.
/// This operation never advances physics or waits for network exchange.
///
/// # Safety
/// Every non-null pointer must reference a live, aligned allocation of at least
/// its declared element count for this call. Input and output allocations must
/// not overlap; no other thread may mutate them during the call. Handles are
/// integer IDs and are validated; they are never dereferenced as pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nm_reference_status(handle: u64, output: *mut u8, capacity: u32) -> i32 {
    guarded(|| {
        if output.is_null() || capacity > 65536 {
            return Err("invalid status buffer".into());
        }
        let state = session(handle)?;
        let s = state.lock().map_err(|_| "session unavailable")?;
        if s.closed {
            return Err("closed session".into());
        }
        let bytes = serde_json::to_vec(&serde_json::json!({
            "schema_version": 1, "mode": if s.actuation.npc_control{"programmed-NPC"}else if s.client.is_some(){"AER1-reference"}else{"silent-mechanics"}, "source_step": s.last_step,
            "reset_epoch": s.reset_epoch,
            "profile_id": s.model.profile_id, "model_digest": s.model.digest, "io_digest": s.model.io.digest,
            "brain_id": s.binding.as_ref().map(|b| &b.brain_id),
            "brain_generation": s.binding.as_ref().map(|b| b.brain_generation),
            "disarmed": s.actuation.disarmed, "inhibited": s.actuation.actuation_inhibited,
            "applied": s.actuation.applied, "expired": s.actuation.expired,
            "npc_steps":s.actuation.npc_steps,"npc_policy":aarnn_embodiment::npc::POLICY,
            "controller_mode":if s.actuation.npc_control{"programmed-NPC"}else if s.client.is_some(){"AER1-reference"}else{"silent-mechanics"},
            "transport": s.client.as_ref().map(Client::status),
        })).map_err(|e| e.to_string())?;
        if bytes.len() > capacity as usize {
            return Err("status buffer too small".into());
        }
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), output, bytes.len());
        }
        Ok(bytes.len() as i32)
    })
}
/// Select exclusive control: 0=automatic NPC/ready AARNN, 1=NPC only,
/// 2=silent mechanics, 3=AARNN only. Explicit stop/reset latches remain in force.
///
/// # Safety
/// Integer handles are validated and never dereferenced as pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nm_control_mode(handle: u64, mode: u32) -> i32 {
    guarded(|| {
        use aarnn_embodiment::npc::Control;
        let mode = match mode {
            0 => Control::Auto,
            1 => Control::Npc,
            2 => Control::Silent,
            3 => Control::Aarnn,
            _ => return Err("unknown control mode".into()),
        };
        let state = session(handle)?;
        let mut s = state.lock().map_err(|_| "session unavailable")?;
        if s.closed {
            return Err("closed controller".into());
        }
        let npc = mode.npc(s.client.is_some())?;
        s.actuation.disarm();
        s.npc_enabled = npc;
        Ok(0)
    })
}
/// Copy one active-drive flag per actuator. A numeric zero target may still
/// enable a position servo; silence must disable its gain, not servo to zero.
///
/// # Safety
/// Output must be a live writable buffer of capacity bytes for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nm_reference_motor_mask(
    handle: u64,
    output: *mut u8,
    capacity: u32,
) -> i32 {
    guarded(|| {
        if output.is_null() || capacity > NM_MAX_CHANNELS {
            return Err("invalid motor mask buffer".into());
        }
        unsafe {
            std::ptr::write_bytes(output, 0, capacity as usize);
        }
        let state = session(handle)?;
        let s = state.lock().map_err(|_| "session unavailable")?;
        if s.closed || capacity as usize != s.model.actuators.len() {
            return Err("motor mask shape".into());
        }
        for i in 0..capacity as usize {
            unsafe {
                *output.add(i) = u8::from(s.actuation.active(i));
            }
        }
        Ok(0)
    })
}
/// Project one bounded batch of native measurements with the shared Rust
/// ordering/scaling/history. See projection::Projection::width for layout.
/// Returns the normalized sensor count. This does not submit AER or step time.
///
/// # Safety
/// Buffers must be aligned, live, sized by their element counts and nonoverlapping.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nm_reference_project(
    handle: u64,
    measurements: *const f64,
    measurement_count: u32,
    sensors: *mut f64,
    sensor_capacity: u32,
) -> i32 {
    guarded(|| {
        if !sensors.is_null() && sensor_capacity <= NM_MAX_CHANNELS {
            unsafe {
                std::ptr::write_bytes(sensors, 0, sensor_capacity as usize);
            }
        }
        let state = session(handle)?;
        let mut s = state.lock().map_err(|_| "session unavailable")?;
        let result = (|| -> Result<i32, String> {
            if s.closed
                || measurements.is_null()
                || sensors.is_null()
                || measurement_count > NM_MAX_CHANNELS
                || sensor_capacity as usize != s.model.io.sensors.len()
            {
                return Err("invalid measurement buffers".into());
            }
            let raw =
                unsafe { std::slice::from_raw_parts(measurements, measurement_count as usize) };
            let Session {
                model, projection, ..
            } = &mut *s;
            let out = projection.measure(model, raw)?;
            unsafe {
                std::ptr::copy_nonoverlapping(out.as_ptr(), sensors, out.len());
            }
            Ok(out.len() as i32)
        })();
        if result.is_err() {
            s.actuation.stop();
        }
        result
    })
}
/// Batched dissipative panel forces. Each panel input is velocity XYZ (m/s),
/// row-major local-to-world rotation (nine doubles), area (m²). Output is world
/// force XYZ (N). At most 32 panels; no engine- or spike-specific lift shortcut.
///
/// # Safety
/// Every non-null pointer must reference a live, aligned allocation of at least
/// its declared element count for this call. Input and output allocations must
/// not overlap; no other thread may mutate them during the call. Handles are
/// integer IDs and are validated; they are never dereferenced as pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nm_panel_forces(
    inputs: *const f64,
    panel_count: u32,
    outputs: *mut f64,
    output_capacity: u32,
) -> i32 {
    guarded(|| {
        if inputs.is_null()
            || outputs.is_null()
            || panel_count > 32
            || output_capacity != panel_count * 3
        {
            return Err("invalid panel batch".into());
        }
        let values = unsafe { std::slice::from_raw_parts(inputs, panel_count as usize * 13) };
        let mut result = Vec::with_capacity(output_capacity as usize);
        for p in values.chunks_exact(13) {
            let r = std::array::from_fn(|i| std::array::from_fn(|j| p[3 + i * 3 + j]));
            result
                .extend(force::panel_force([p[0], p[1], p[2]], r, p[12]).map_err(str::to_string)?);
        }
        unsafe {
            std::ptr::copy_nonoverlapping(result.as_ptr(), outputs, result.len());
        }
        Ok(0)
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_force_abi_and_closed_handles_fail() {
        unsafe {
            let input = [8., 0., -2., 1., 0., 0., 0., 1., 0., 0., 0., 1., 0.045];
            let mut out = [0.; 3];
            assert_eq!(nm_embodiment_version(), 1);
            assert_eq!(nm_panel_forces(input.as_ptr(), 1, out.as_mut_ptr(), 3), 0);
            assert!(out[0] * 8. + out[2] * -2. <= 1e-12);
            assert!(nm_panel_forces(std::ptr::null(), 1, out.as_mut_ptr(), 3) < 0);
            assert!(nm_reference_close(u64::MAX) < 0);
        }
    }
    #[test]
    fn foreign_npc_mode_is_separate_and_stop_wins() {
        unsafe {
            let p = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../sim/content/embodiments/generated/ugot_mecanum.xml");
            let p = p.to_str().unwrap().as_bytes();
            let mut h = 0;
            assert_eq!(
                nm_reference_open(p.as_ptr(), p.len() as u32, std::ptr::null(), 0, &mut h),
                0
            );
            assert_eq!(nm_control_mode(h, 0), 0);
            let sensors = [1.; 19];
            let mut out = [0.; 6];
            assert_eq!(
                nm_reference_step(h, 1, sensors.as_ptr(), 19, out.as_mut_ptr(), 6),
                2
            );
            assert!(out[0] > 0.);
            assert_eq!(nm_reference_disarm(h), 0);
            assert_eq!(
                nm_reference_step(h, 2, sensors.as_ptr(), 19, out.as_mut_ptr(), 6),
                0
            );
            assert_eq!(out, [0.; 6]);
            assert_eq!(nm_reference_close(h), 0);
        }
    }
    #[test]
    fn graph_and_local_session_abi_rejects_mapping_replay() {
        unsafe {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            let path = root.join("sim/content/embodiments/generated/ugot_mecanum.xml");
            let graph = root.join("sim/content/embodiments/graphs/ugot_mecanum.json");
            let p = path.to_str().unwrap().as_bytes();
            let g = graph.to_str().unwrap().as_bytes();
            assert_eq!(
                nm_embodiment_verify_graph(p.as_ptr(), p.len() as u32, g.as_ptr(), g.len() as u32),
                0
            );
            let mut handle = 0;
            assert_eq!(
                nm_reference_open(p.as_ptr(), p.len() as u32, std::ptr::null(), 0, &mut handle),
                0
            );
            let values = [0.; 19];
            let mut controls = [1.; 6];
            assert_eq!(
                nm_reference_step(handle, 1, values.as_ptr(), 19, controls.as_mut_ptr(), 6),
                0
            );
            assert_eq!(controls, [0.; 6]);
            assert!(
                nm_reference_step(handle, 1, values.as_ptr(), 19, controls.as_mut_ptr(), 6) < 0
            );
            assert_eq!(controls, [0.; 6]);
            let mut buffer = [0u8; 8192];
            let len = nm_reference_status(handle, buffer.as_mut_ptr(), 8192);
            assert!(len > 0);
            let status: serde_json::Value =
                serde_json::from_slice(&buffer[..len as usize]).unwrap();
            assert_eq!(status["inhibited"], true);
            assert_eq!(status["source_step"], 1);
            assert_eq!(nm_reference_disarm(handle), 0);
            assert_eq!(nm_reference_close(handle), 0);
            assert!(nm_reference_disarm(handle) < 0);
        }
    }
}
