use crate::hardware::gpio::{GpioBackend, GpioEdge};
use crate::time::unix_time_ns;
use async_trait::async_trait;
use parking_lot::Mutex;
use std::collections::VecDeque;
use tracing::info;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockPulse {
    pub line: String,
    pub pulse_width_ns: u32,
    pub value: u32,
}

#[derive(Default)]
pub struct MockGpioBackend {
    edges: Mutex<VecDeque<GpioEdge>>,
    pulses: Mutex<Vec<MockPulse>>,
}

impl MockGpioBackend {
    pub fn inject_edge(&self, line: impl Into<String>, rising: bool) {
        self.edges.lock().push_back(GpioEdge {
            line: line.into(),
            rising,
            timestamp_ns: unix_time_ns(),
        });
    }

    pub fn pulses(&self) -> Vec<MockPulse> {
        self.pulses.lock().clone()
    }
}

#[async_trait]
impl GpioBackend for MockGpioBackend {
    async fn emit_pulse(&self, line: &str, pulse_width_ns: u32, value: u32) -> anyhow::Result<()> {
        info!(
            line = line,
            pulse_width_ns = pulse_width_ns,
            value = value,
            "mock gpio emit pulse"
        );
        self.pulses.lock().push(MockPulse {
            line: line.to_string(),
            pulse_width_ns,
            value,
        });
        Ok(())
    }

    async fn read_edges(&self) -> anyhow::Result<Vec<GpioEdge>> {
        let mut edges = self.edges.lock();
        Ok(edges.drain(..).collect())
    }
}
