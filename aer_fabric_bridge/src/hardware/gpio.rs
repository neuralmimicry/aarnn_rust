use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct GpioEdge {
    pub line: String,
    pub rising: bool,
    pub timestamp_ns: u64,
}

#[async_trait]
pub trait GpioBackend: Send + Sync {
    async fn emit_pulse(&self, line: &str, pulse_width_ns: u32, value: u32) -> anyhow::Result<()>;
    async fn read_edges(&self) -> anyhow::Result<Vec<GpioEdge>>;
}
