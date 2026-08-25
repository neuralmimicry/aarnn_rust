//! Bounded JNI control surface for the Android application.
//!
//! Only versioned control/checkpoint operations cross this boundary. High-rate
//! AER and media traffic must use bounded batches at a later governed adapter;
//! no Rust reference, lock or panic is allowed to cross JNI. Handles are kept
//! in a validated process-local registry so malformed/stale handles fail with
//! an error code instead of being dereferenced as pointers.

use crate::deterministic::BrainId;
use crate::engine::EngineSpec;
use crate::mobile_runtime::{MobileCheckpoint, MobileExecutionMode, MobileRuntime};
use jni::JNIEnv;
use jni::objects::{JByteArray, JClass, JString};
use jni::sys::{jbyte, jbyteArray, jint, jlong};
use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Mutex, OnceLock};

const ABI_VERSION: jint = 1;
const MAX_INPUT_BYTES: usize = 64 * 1024;

static NEXT_HANDLE: AtomicI64 = AtomicI64::new(1);
static RUNTIMES: OnceLock<Mutex<BTreeMap<jlong, MobileRuntime>>> = OnceLock::new();

fn runtimes() -> &'static Mutex<BTreeMap<jlong, MobileRuntime>> {
    RUNTIMES.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn with_runtime<T>(
    handle: jlong,
    f: impl FnOnce(&mut MobileRuntime) -> Result<T, ()>,
) -> Result<T, ()> {
    let mut runtimes = runtimes().lock().map_err(|_| ())?;
    let runtime = runtimes.get_mut(&handle).ok_or(())?;
    f(runtime)
}

fn result_code(result: Result<(), ()>) -> jint {
    result.map_or(-1, |_| 0)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_neuralmimicry_aarnn_NativeBridge_nativeAbiVersion(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    ABI_VERSION
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_neuralmimicry_aarnn_NativeBridge_nativeCreate(
    mut env: JNIEnv,
    _class: JClass,
    brain_id: jlong,
    spec_json: JString,
) -> jlong {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let brain = BrainId::new(u64::try_from(brain_id).map_err(|_| ())?).map_err(|_| ())?;
        let spec_text = env.get_string(&spec_json).map_err(|_| ())?;
        let spec: EngineSpec =
            serde_json::from_str(spec_text.to_str().map_err(|_| ())?).map_err(|_| ())?;
        let runtime = MobileRuntime::new(brain, MobileExecutionMode::StandaloneBrain, spec)
            .map_err(|_| ())?;
        let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
        if handle <= 0 {
            return Err(());
        }
        runtimes().lock().map_err(|_| ())?.insert(handle, runtime);
        Ok(handle)
    }));
    match result {
        Ok(Ok(handle)) => handle,
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_neuralmimicry_aarnn_NativeBridge_nativeRestore(
    env: JNIEnv,
    _class: JClass,
    checkpoint: JByteArray,
) -> jlong {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let length = env.get_array_length(&checkpoint).map_err(|_| ())? as usize;
        if length > crate::mobile_runtime::MAX_MOBILE_CHECKPOINT_BYTES {
            return Err(());
        }
        let mut bytes = vec![0 as jbyte; length];
        env.get_byte_array_region(&checkpoint, 0, &mut bytes)
            .map_err(|_| ())?;
        let bytes = bytes.into_iter().map(|byte| byte as u8).collect::<Vec<_>>();
        let checkpoint = MobileCheckpoint::decode(&bytes).map_err(|_| ())?;
        let runtime = MobileRuntime::restore(checkpoint).map_err(|_| ())?;
        let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
        if handle <= 0 {
            return Err(());
        }
        runtimes().lock().map_err(|_| ())?.insert(handle, runtime);
        Ok(handle)
    }));
    match result {
        Ok(Ok(handle)) => handle,
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_neuralmimicry_aarnn_NativeBridge_nativeInitialise(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jint {
    let result = catch_unwind(AssertUnwindSafe(|| {
        with_runtime(handle, |runtime| runtime.initialise().map_err(|_| ()))
    }));
    result_code(result.unwrap_or(Err(())).map(|_| ()))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_neuralmimicry_aarnn_NativeBridge_nativeStart(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jint {
    let result = catch_unwind(AssertUnwindSafe(|| {
        with_runtime(handle, |runtime| runtime.start().map_err(|_| ()))
    }));
    result_code(result.unwrap_or(Err(())).map(|_| ()))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_neuralmimicry_aarnn_NativeBridge_nativePause(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jint {
    let result = catch_unwind(AssertUnwindSafe(|| {
        with_runtime(handle, |runtime| runtime.pause().map_err(|_| ()))
    }));
    result_code(result.unwrap_or(Err(())).map(|_| ()))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_neuralmimicry_aarnn_NativeBridge_nativeEnterForeground(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jint {
    let result = catch_unwind(AssertUnwindSafe(|| {
        with_runtime(handle, |runtime| runtime.enter_foreground().map_err(|_| ()))
    }));
    result_code(result.unwrap_or(Err(())).map(|_| ()))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_neuralmimicry_aarnn_NativeBridge_nativeEnterBackground(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jbyteArray {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let checkpoint = with_runtime(handle, |runtime| {
            runtime
                .enter_background()
                .map_err(|_| ())?
                .encode()
                .map_err(|_| ())
        })?;
        env.byte_array_from_slice(&checkpoint)
            .map(|array| array.into_raw())
            .map_err(|_| ())
    }));
    match result {
        Ok(Ok(array)) => array,
        _ => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_neuralmimicry_aarnn_NativeBridge_nativeStep(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
    input: JByteArray,
) -> jint {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let length = env.get_array_length(&input).map_err(|_| ())? as usize;
        if length > MAX_INPUT_BYTES {
            return Err(());
        }
        let mut values = vec![0 as jbyte; length];
        env.get_byte_array_region(&input, 0, &mut values)
            .map_err(|_| ())?;
        with_runtime(handle, |runtime| {
            runtime.step(Some(&values)).map(|_| ()).map_err(|_| ())
        })
    }));
    result_code(result.unwrap_or(Err(())))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_neuralmimicry_aarnn_NativeBridge_nativeCheckpoint(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jbyteArray {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let checkpoint = with_runtime(handle, |runtime| runtime.checkpoint().map_err(|_| ()))?;
        let bytes = checkpoint.encode().map_err(|_| ())?;
        env.byte_array_from_slice(&bytes)
            .map(|array| array.into_raw())
            .map_err(|_| ())
    }));
    match result {
        Ok(Ok(array)) => array,
        _ => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_neuralmimicry_aarnn_NativeBridge_nativeDestroy(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jint {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let mut runtime = runtimes()
            .lock()
            .map_err(|_| ())?
            .remove(&handle)
            .ok_or(())?;
        runtime.terminate().map_err(|_| ())
    }));
    result_code(result.unwrap_or(Err(())))
}
