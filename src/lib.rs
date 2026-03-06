//! # Neuromorphic Demo Library
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

/// Configuration structures for neurons, learning rules, and network topology.
pub mod config;
/// Core neural network data structures including layers, neurons, and synapses.
pub mod network;
/// Simulation engine responsible for time-stepping and state updates.
pub mod sim;
/// Address-Event Representation (AER) encoding/decoding.
pub mod aer;
/// Optional AER <-> CAN conversion helpers for robotic endpoints.
pub mod aer_can;
/// UDP-based AER stimuli IO bridge.
pub mod stimuli;

#[cfg(feature = "robot_io")]
/// Bridge for interfacing with external robotic systems or simulators (e.g., Webots).
pub mod bridge;

/// Orchestration logic for running simulations in various modes.
pub mod runner;
/// Components for distributed simulation across multiple nodes.
pub mod distributed;
/// Remote Direct Memory Access (RDMA) backend for low-latency distributed communication.
pub mod rdma;

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
