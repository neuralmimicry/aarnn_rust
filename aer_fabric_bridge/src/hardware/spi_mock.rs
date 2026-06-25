use crate::hardware::spi::SpiBackend;
use anyhow::bail;
use async_trait::async_trait;
use parking_lot::Mutex;
use tracing::debug;

pub struct MockSpiBackend {
    pub probe_ok: bool,
    writes: Mutex<Vec<Vec<u8>>>,
}

impl Default for MockSpiBackend {
    fn default() -> Self {
        Self {
            probe_ok: true,
            writes: Mutex::new(Vec::new()),
        }
    }
}

impl MockSpiBackend {
    pub fn with_probe_ok(probe_ok: bool) -> Self {
        Self {
            probe_ok,
            writes: Mutex::new(Vec::new()),
        }
    }

    pub fn writes(&self) -> Vec<Vec<u8>> {
        self.writes.lock().clone()
    }
}

#[async_trait]
impl SpiBackend for MockSpiBackend {
    async fn transfer(&self, bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
        debug!(bytes = bytes.len(), "mock spi transfer");
        self.writes.lock().push(bytes.to_vec());
        Ok(bytes.to_vec())
    }

    async fn write(&self, bytes: &[u8]) -> anyhow::Result<()> {
        debug!(bytes = bytes.len(), "mock spi write");
        self.writes.lock().push(bytes.to_vec());
        Ok(())
    }

    async fn probe_device(&self) -> anyhow::Result<()> {
        if self.probe_ok {
            Ok(())
        } else {
            bail!("mock spi probe configured as unavailable")
        }
    }
}
