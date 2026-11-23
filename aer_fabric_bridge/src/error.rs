// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BridgeError {
    #[error("invalid packet: {0}")]
    InvalidPacket(String),
    #[error("configuration error: {0}")]
    Config(String),
    #[error("IO error: {0}")]
    Io(String),
}
