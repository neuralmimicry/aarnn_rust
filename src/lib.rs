//! # AARNN Library
//!
//! This library provides the core engine for the neuromorphic simulation project.
//! While the primary entry point is the binary in `src/main.rs`, this library
//! exposes internal modules to support:
//! 1. Integration testing
//! 2. Usage examples (found in `examples/`)
//! 3. Foreign Function Interface (FFI) for C++/Python integration
//!
//! ## Workflow
//! Typically, a simulation is configured via `config`, a `network` is constructed,
//! and the `sim` module handles the execution of the neural dynamics over time.
//! `runner` provides higher-level orchestration, while modules like `distributed`,
//! `rdma`, and `shmem` support scaling across multiple processes or nodes.

#[macro_use]
/// Observability tools for logging, probing, and data export.
pub mod obs;

/// AARNN-specific algorithm kernels partitioned into replaceable modules.
pub mod aarnn;
/// Address-Event Representation (AER) encoding/decoding.
pub mod aer;
/// Optional AER <-> CAN conversion helpers for robotic endpoints.
pub mod aer_can;
/// Shared filesystem-backed auth/session stores used by scaled frontends.
pub mod auth_store;
/// Configuration structures for neurons, learning rules, and network topology.
pub mod config;
/// User-agnostic engine facade around `Runner`.
pub mod engine;
/// Core neural network data structures including layers, neurons, and synapses.
pub mod network;
pub mod nmchain;
/// Simulation engine responsible for time-stepping and state updates.
pub mod sim;
/// Shared spike input/output encoders, transports, and profile-specific adapters.
pub mod spike_io;
/// UDP-based AER stimuli IO bridge.
pub mod stimuli;

#[cfg(feature = "robot_io")]
/// Bridge for interfacing with external robotic systems or simulators (e.g., Webots).
pub mod bridge;

/// Components for distributed simulation across multiple nodes.
pub mod distributed;
#[cfg(feature = "openmpi")]
/// OpenMPI bootstrap and transport helpers.
pub mod openmpi_runtime;
/// Remote Direct Memory Access (RDMA) backend for low-latency distributed communication.
pub mod rdma;
/// Orchestration logic for running simulations in various modes.
pub mod runner;
/// Persistent runtime middleware for multi-user workspaces and scheduling.
pub mod runtime;
/// Shared request/response models and clients for runtime-facing frontends.
pub mod runtime_api;
/// Shared file/lease primitives for runtime coordination on PVC-backed deployments.
pub mod shared_fs;

#[cfg(feature = "growth3d")]
/// Topological and spatial layout definitions for 3D neural growth.
pub mod topology;

#[cfg(feature = "morpho")]
/// Morphological growth and developmental simulation logic.
pub mod morphology;

#[cfg(feature = "ui")]
/// Data providers for the real-time visualization UI.
pub mod providers;

#[cfg(feature = "ui")]
/// Real-time visualization interface.
pub mod ui;

#[cfg(feature = "opencl")]
/// Backend-agnostic GPU runtime facade used by OpenCL/CUDA execution.
pub mod gpu_api;

#[cfg(feature = "opencl")]
/// OpenCL kernels and host-side drivers for GPGPU acceleration.
pub mod cl_compute;

#[cfg(feature = "ffi_bridge")]
/// C-compatible interfaces for external language bindings.
pub mod ffi;

#[cfg(feature = "shmem")]
/// Shared memory communication primitives for high-performance inter-process data exchange.
pub mod shmem;

/// Genetic Algorithm for parameter optimization.
pub mod ga;

/// Resource and thermal monitoring.
pub mod monitor;

/// Optional CPU-core affinity helpers for proactive thread distribution.
pub mod affinity;
