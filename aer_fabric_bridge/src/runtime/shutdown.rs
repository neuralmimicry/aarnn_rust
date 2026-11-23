// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
use tokio::sync::watch;

pub fn channel() -> (watch::Sender<bool>, watch::Receiver<bool>) {
    watch::channel(false)
}
