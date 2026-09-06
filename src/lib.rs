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
/// Versioned bounded AER-over-transport session protocol.
pub mod aer_transport;
/// Shared filesystem-backed auth/session stores used by scaled frontends.
pub mod auth_store;
/// Single-writer authoritative shard boundary for durable causal execution.
pub mod authoritative_shard;
/// Whole-brain transfer session composing stable executor sources, durable
/// destination reconstruction, group fencing and placement publication.
pub mod brain_migration_session;
/// Component-scoped local causal reference executor.
pub mod causal;
/// Generated causal gRPC conversion and bounded stream validation seam.
pub mod causal_transport;
/// Shared Refiner/Postgres-backed auth and token client helpers.
pub mod central_auth;
/// Versioned, bounded network transfer and immutable publication of stable
/// executor checkpoint sets.
pub mod checkpoint_transfer;
/// Deterministic assembly and validation of complete cluster shard snapshots.
pub mod cluster_snapshot;
/// Configuration structures for neurons, learning rules, and network topology.
pub mod config;
/// Asynchronous GVT and distributed consistent-cut coordination.
pub mod consistent_cut;
/// Reliable bounded causal data-plane reference types.
pub mod data_plane;
/// Deployment modes, topology intent, and infrastructure autodetection helpers.
pub mod deployment;
/// Stable identities, superdense logical time and deterministic reference primitives.
pub mod deterministic;
/// Fenced causal WAL and immutable checkpoint reference storage.
pub mod durability;
/// User-agnostic engine facade around `Runner`.
pub mod engine;
/// Explicitly consented independent-brain federation links.
pub mod federation;
/// Explicit bounded global-field events for causal execution.
pub mod field_events;
/// FPAA discovery, verification, and routing helpers.
pub mod fpaa;
/// Rust output generated from the versioned management protobuf schema.
pub mod generated_management;
/// Opt-in live managed-network durability owner.
pub mod managed_durability;
/// Bounded placement-aware runtime for partial stable-shard workers.
pub mod managed_partial_shard_runtime;
/// Runtime ownership and generation-admission boundary for stable virtual shards.
pub mod managed_shard_runtime;
/// Explicitly registered stable-ID executor adapter for the managed node loop.
#[cfg(feature = "stable_executor_live")]
pub mod managed_stable_executor;
/// Orchestrator-authorised versioned management reference types.
pub mod management;
/// Staged migration, canary and rollback evidence.
pub mod migration;
/// Quorum lease, transfer and registry cutover composition.
pub mod migration_coordinator;
/// Orchestrator-owned registration and asynchronous dispatch for live
/// migration executors.
pub mod migration_executor;
/// Brain-wide barrier for multi-shard migration cutovers.
pub mod migration_group;
/// Durable, fenced brain-wide migration operation journal.
pub mod migration_operation;
/// Bounded checkpoint transfer and placement cutover evidence.
pub mod migration_transfer;
#[cfg(feature = "mobile_android")]
/// Bounded JNI control surface for the official Android shell.
pub mod mobile_ffi;
/// Platform-neutral mobile lifecycle, checkpoint and capability contracts.
pub mod mobile_runtime;
/// Independent-brain fair scheduler and resource placement reference types.
pub mod multi_brain;
/// Core neural network data structures including layers, neurons, and synapses.
pub mod network;
pub(crate) mod neuron_kernels;
pub mod nmchain;
/// Shared mTLS/session identity checks for inter-node transport.
pub(crate) mod node_auth;
/// Bounded partial-worker adapter for physically distributed virtual shards.
pub mod partial_shard_executor;
/// Governed peripheral sessions, USB AER admission and fenced effects.
pub mod peripheral;
/// Deterministic virtual-shard placement and fenced migration contracts.
pub mod placement;
/// Live stable-executor placement reconciliation and per-node activation
/// command generation. This remains behind the explicit live-executor gate.
#[cfg(feature = "stable_executor_live")]
pub mod placement_automation;
/// Deterministic hysteresis, budget and residence gate for automatic movement.
pub mod placement_controller;
/// Authoritative stable-shard placement registry and crash-safe apply boundary.
pub mod placement_registry;
/// Deterministic failover/rejoin and RPO/RTO evidence contracts.
pub mod recovery;
/// Scientific/numerical validation profiles and reproducible reports.
pub mod scientific_validation;
/// Shared service-visibility and authorisation helpers used across browser and API surfaces.
pub mod service_access;
/// Deterministic multi-shard stable-ID biological execution fabric.
pub mod shard_executor;
/// Simulation engine responsible for time-stepping and state updates.
pub mod sim;
/// Shared spike input/output encoders, transports, and profile-specific adapters.
pub mod spike_io;
/// Fenced transactional commit boundary for stable-executor rehearsals.
pub mod stable_executor_authority;
/// Durable actor handoff for stable multi-shard executor cuts.
pub mod stable_executor_durable;
/// Immutable filesystem checkpoint sets for the stable multi-shard executor.
pub mod stable_executor_store;
/// Crash-safe per-destination handoff log for physically distributed shards.
pub mod stable_outbound;
/// Explicit versioned bootstrap and recovery contract for stable runtimes.
#[cfg(feature = "stable_executor_live")]
pub mod stable_runtime_bootstrap;
/// Placement-authorised concurrent dispatch over durable stable-shard streams.
pub mod stable_shard_dispatch;
/// Versioned durable protobuf data plane for physically distributed stable
/// shard workers.
pub mod stable_shard_transport;
/// Stable-ID executor worker capability and registration contract.
pub mod stable_worker;
/// UDP-based AER stimuli IO bridge.
pub mod stimuli;
#[cfg(feature = "superdense_executor")]
/// Feature-gated local superdense adapter for the legacy biological kernel.
pub mod superdense;

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

/// Versioned biological topology and conservative component planning.
pub mod topology_model;

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
