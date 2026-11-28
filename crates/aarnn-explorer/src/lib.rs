// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! AARNN connectome explorer: import, complete activity capture, GNAT
//! analysis, isolated interventions, functional and resilience evaluation,
//! function-preserving reduction and executable export.
//!
//! The library holds the testable logic; `src/main.rs` is the CLI. Simulation
//! always goes through `aarnn_rust::runner::Runner` inside worker processes.

pub mod api;
pub mod author;
pub mod capture;
pub mod config;
pub mod doctor;
pub mod error;
pub mod exec;
pub mod gnat;
pub mod jobs;
pub mod lesion;
pub mod library;
pub mod model;
pub mod motifs;
pub mod optimise;
pub mod pipeline;
pub mod slurm;
pub mod stats;
pub mod store;
pub mod tasks;
#[cfg(feature = "web-ui")]
pub mod web;
pub mod worker;
