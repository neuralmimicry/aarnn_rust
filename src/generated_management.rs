//! Generated management-contract boundary.
//!
//! The source of truth is `proto/management.proto`; `build.rs` regenerates
//! Rust clients and servers on every build. No generated output is edited by
//! hand or committed as a second schema.
// management-schema-source-digest:bd43399d784f092c

pub const MANAGEMENT_SCHEMA_VERSION: u32 = 2;

pub mod proto {
    tonic::include_proto!("management.v1");
}
