// Minimal C ABI for neuromorphic_demo FFI bridge (Linux/C++/Clang)
// Build the shared library with:
//   cargo build --release --features ffi_bridge
// The resulting lib: target/release/libneuromorphic_demo.so
//
// Threading model:
// - All functions are non-reentrant. Call from a single thread.
// - Initialize once with nm_init(); call nm_shutdown() at process end.
//
// Buffer contracts:
// - nm_set_port_by_index writes [len] floats starting at sensory index [start].
// - nm_get_port_by_index reads [len] floats starting at output index [start].
// - Indices must be within the ranges configured at nm_init.
// - All floats are little-endian IEEE-754.

#pragma once

#ifdef __cplusplus
extern "C" {
#endif

#include <stddef.h>

// Initialize the engine with a minimal JSON config, e.g.:
//   {"sensory":25, "output":11}
// Optional runtime tuning via JSON:
//   - "threshold": float, spike threshold in [0,1] (default 0.5), e.g. {"sensory":25,"output":11,"threshold":0.2}
// Returns 0 on success; negative on error.
int nm_init(const char* config_json);

// Write sensor values directly by index range. Returns 0 on success.
int nm_set_port_by_index(size_t start, size_t len, const float* data);

// Read actuator values directly by index range. Returns 0 on success.
int nm_get_port_by_index(size_t start, size_t len, float* out);

// Advance one simulation step at time t_ms. Returns 0 on success.
int nm_step(double t_ms);

// Set spike quantizer threshold at runtime. Returns 0 on success.
int nm_set_quantizer_threshold(float threshold);

// Shutdown (no-op; resources are freed at process exit). Safe to call once.
void nm_shutdown(void);

#ifdef __cplusplus
} // extern "C"
#endif
