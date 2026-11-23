// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Webots platform boundary. No neural executor or production effect authority.
pub mod legacy;
#[cfg(feature = "native")]
pub mod native;
