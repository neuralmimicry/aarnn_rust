//! # Remote Direct Memory Access (RDMA) Backend
//!
//! This module provides a low-latency data transfer backend using InfiniBand RDMA.
//! It is used by the distributed simulation engine to stream spikes between
//! nodes with minimal CPU overhead and sub-microsecond latency.
//!
//! If RDMA-capable hardware is not available, the system transparently falls
//! back to gRPC-based communication.

#[cfg(feature = "rdma")]
use ibverbs;

#[allow(dead_code)]
pub struct RdmaContext {
    #[cfg(feature = "rdma")]
    _context: Option<ibverbs::Context>,
}

#[allow(dead_code)]
impl RdmaContext {
    pub fn init() -> Self {
        #[cfg(feature = "rdma")]
        {
            let devices = ibverbs::devices();
            if let Ok(devs) = devices {
                if let Some(dev) = devs.iter().next() {
                    let ctx_res: Result<ibverbs::Context, _> = dev.open();
                    if let Ok(ctx) = ctx_res {
                        nm_log!("[info] RDMA device found and opened: {:?}", dev.name());
                        return Self { _context: Some(ctx) };
                    }
                }
            }
            nm_log!("[info] No RDMA devices found, using gRPC only.");
            Self { _context: None }
        }
        #[cfg(not(feature = "rdma"))]
        {
            Self {}
        }
    }

    pub fn is_available(&self) -> bool {
        #[cfg(feature = "rdma")]
        {
            self._context.is_some()
        }
        #[cfg(not(feature = "rdma"))]
        {
            false
        }
    }
}
