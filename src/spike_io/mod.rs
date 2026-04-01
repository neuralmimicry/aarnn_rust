//! Shared spike input/output boundary for external systems.
//!
//! This module separates three concerns that were previously spread across the
//! UI, robot bridge, and distributed transport code:
//! - `encoding`: signal-domain encoders/decoders (digital, analog, temporal, population).
//! - `transport`: frame helpers for AER payloads, indices, and hex exchange.
//! - `profiles`: network-specific ingress/egress policies (for example C. elegans,
//!   Drosophila, NAO, or generic layouts).

pub mod encoding;
pub mod profiles;
pub mod transport;
