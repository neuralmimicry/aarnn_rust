//! AARNN-specific algorithm partitions.
//!
//! The broader project mixes platform concerns (UI, Webots, distributed transport,
//! operators, deployment) with the neural engine itself. This namespace isolates the
//! software reference implementations of the biologically motivated AARNN kernels so
//! they can be replaced one block at a time by future hardware backends.
//!
//! The boundaries are intentionally aligned with likely FPAA substitution points:
//! - `dynamics`: weighted current summation, synaptic filtering, gap coupling,
//!   diffusion-like field modulation, and active dendritic shaping.
//! - `plasticity`: short-term plasticity, long-timescale weight constraints, and
//!   probabilistic release logic.
//! - `transmission`: morphology-aware path delay and attenuation.
//!
//! In software these modules are pure or near-pure functions. In an FPAA-oriented
//! deployment the same seams can be mapped to configurable analog blocks, floating-gate
//! vector-matrix multipliers, OTA-C filters, capacitor state, diffusor meshes, or a
//! hybrid analog-plus-supervisory control loop.

/// Current-domain and state-update primitives for AARNN signal propagation.
pub mod dynamics;
/// Synaptic resource updates, release modeling, and long-timescale plasticity constraints.
pub mod plasticity;
/// Morphology-aware conduction delay and attenuation utilities.
pub mod transmission;
