use async_trait::async_trait;

#[async_trait]
pub trait SpiBackend: Send + Sync {
    async fn transfer(&self, bytes: &[u8]) -> anyhow::Result<Vec<u8>>;
    async fn write(&self, bytes: &[u8]) -> anyhow::Result<()>;

    async fn probe_device(&self) -> anyhow::Result<()> {
        let _ = self.transfer(&[0x00]).await?;
        Ok(())
    }
}
