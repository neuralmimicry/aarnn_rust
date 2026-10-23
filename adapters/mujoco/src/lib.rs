//! MuJoCo platform adapter. No neural kernel, UI or production authorisation.
//! Native FFI stays in one audited ownership boundary; portable contracts can
//! be tested without an installed physics SDK.
pub use aarnn_embodiment::{aer, force, schema};
#[cfg(feature = "native")]
pub mod native;

#[cfg(feature = "native")]
pub mod legacy;
#[cfg(feature = "native")]
pub mod world;
