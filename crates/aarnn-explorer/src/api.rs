// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Loopback interactive UI and job API (`/api/explorer/v1`), feature
//! `web-ui`. Without the feature, `serve` reports an unsupported capability.

use std::path::Path;

use serde_json::Value;

#[cfg(not(feature = "web-ui"))]
use crate::error::ExplorerError;
use crate::error::Result;

#[cfg(not(feature = "web-ui"))]
pub fn serve(_project: &Path, _listen: &str, _open: bool) -> Result<Value> {
    Err(ExplorerError::Unsupported(
        "built without the web-ui feature; rebuild with --features web-ui".into(),
    ))
}

#[cfg(feature = "web-ui")]
pub fn serve(project: &Path, listen: &str, open: bool) -> Result<Value> {
    crate::web::serve(project, listen, open)
}
