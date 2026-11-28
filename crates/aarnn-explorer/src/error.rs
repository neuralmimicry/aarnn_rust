// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Typed errors with stable exit codes (brief Section 15).

use std::fmt;

/// Stable process exit codes. Job success is separate from scientific
/// acceptance: a completed experiment that shows a candidate fails exits with
/// `FailedAcceptance`, not `Internal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    Ok = 0,
    Internal = 1,
    InvalidInput = 2,
    UnsupportedCapability = 3,
    BudgetExhausted = 4,
    Cancelled = 5,
    FailedAcceptance = 6,
    Inconclusive = 7,
    Conflict = 8,
}

#[derive(Debug)]
pub enum ExplorerError {
    InvalidInput(String),
    Unsupported(String),
    Budget(String),
    Cancelled(String),
    Conflict(String),
    Io(String),
    Internal(String),
}

impl ExplorerError {
    pub fn exit_code(&self) -> ExitCode {
        match self {
            ExplorerError::InvalidInput(_) => ExitCode::InvalidInput,
            ExplorerError::Unsupported(_) => ExitCode::UnsupportedCapability,
            ExplorerError::Budget(_) => ExitCode::BudgetExhausted,
            ExplorerError::Cancelled(_) => ExitCode::Cancelled,
            ExplorerError::Conflict(_) => ExitCode::Conflict,
            ExplorerError::Io(_) | ExplorerError::Internal(_) => ExitCode::Internal,
        }
    }

    /// Stable machine-readable error code for JSON summaries.
    pub fn code(&self) -> &'static str {
        match self {
            ExplorerError::InvalidInput(_) => "invalid-input",
            ExplorerError::Unsupported(_) => "unsupported-capability",
            ExplorerError::Budget(_) => "budget-exhausted",
            ExplorerError::Cancelled(_) => "cancelled",
            ExplorerError::Conflict(_) => "conflict",
            ExplorerError::Io(_) => "io",
            ExplorerError::Internal(_) => "internal",
        }
    }
}

impl fmt::Display for ExplorerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExplorerError::InvalidInput(m) => write!(f, "invalid input: {m}"),
            ExplorerError::Unsupported(m) => write!(f, "unsupported capability: {m}"),
            ExplorerError::Budget(m) => write!(f, "budget exhausted: {m}"),
            ExplorerError::Cancelled(m) => write!(f, "cancelled: {m}"),
            ExplorerError::Conflict(m) => write!(f, "conflict: {m}"),
            ExplorerError::Io(m) => write!(f, "i/o: {m}"),
            ExplorerError::Internal(m) => write!(f, "internal: {m}"),
        }
    }
}

impl std::error::Error for ExplorerError {}

impl From<std::io::Error> for ExplorerError {
    fn from(e: std::io::Error) -> Self {
        ExplorerError::Io(e.to_string())
    }
}

impl From<serde_json::Error> for ExplorerError {
    fn from(e: serde_json::Error) -> Self {
        ExplorerError::InvalidInput(format!("json: {e}"))
    }
}

pub type Result<T> = std::result::Result<T, ExplorerError>;

pub fn invalid<T>(msg: impl Into<String>) -> Result<T> {
    Err(ExplorerError::InvalidInput(msg.into()))
}
