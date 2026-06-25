use crate::aer::AerEvent;
use crate::hardware::gpio::GpioBackend;
use crate::hardware::software_kernel::{SoftwareKernel, SoftwareKernelEngine};
use crate::hardware::spi::SpiBackend;
use crate::routing::SynapseEndpoint;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{debug, info, warn};

#[derive(Debug, Clone)]
pub enum StimulateResult {
    HardwarePulse {
        line: String,
        pulse_width_ns: u32,
        value: u32,
    },
    SoftwareKernel {
        kernel: SoftwareKernel,
        output_value: f64,
        delay_ns: u32,
        reason: String,
    },
}

pub struct PikaHat {
    pub fpaa_count: u8,
    pub gpio: Arc<dyn GpioBackend>,
    pub spi: Arc<dyn SpiBackend>,
    pub software_kernels: SoftwareKernelEngine,
    pub force_software_fallback: bool,
    fpaa_available: AtomicBool,
}

impl PikaHat {
    pub fn new(
        fpaa_count: u8,
        gpio: Arc<dyn GpioBackend>,
        spi: Arc<dyn SpiBackend>,
        force_software_fallback: bool,
    ) -> Self {
        Self {
            fpaa_count,
            gpio,
            spi,
            software_kernels: SoftwareKernelEngine::default(),
            force_software_fallback,
            fpaa_available: AtomicBool::new(false),
        }
    }
}

impl PikaHat {
    pub async fn detect_fpaa(&self) -> bool {
        if self.force_software_fallback {
            self.fpaa_available.store(false, Ordering::Relaxed);
            warn!("FPAA software fallback forced by node configuration");
            return false;
        }

        match self.spi.probe_device().await {
            Ok(()) => {
                self.fpaa_available.store(true, Ordering::Relaxed);
                info!(
                    fpaa_count = self.fpaa_count,
                    "FPAA transport probe succeeded"
                );
                true
            }
            Err(err) => {
                self.fpaa_available.store(false, Ordering::Relaxed);
                warn!(
                    error = %err,
                    "FPAA transport probe failed; local actions will use software kernels"
                );
                false
            }
        }
    }

    pub fn fpaa_available(&self) -> bool {
        self.fpaa_available.load(Ordering::Relaxed)
    }

    pub fn mark_fpaa_unavailable(&self) {
        self.fpaa_available.store(false, Ordering::Relaxed);
    }

    pub async fn configure_fpaas(&self, ahf_paths: &[PathBuf]) -> anyhow::Result<()> {
        info!(
            fpaa_count = self.fpaa_count,
            ahf_count = ahf_paths.len(),
            "configure_fpaas called (stub)"
        );
        Ok(())
    }

    pub async fn stimulate_endpoint(
        &self,
        endpoint: &SynapseEndpoint,
        event: &AerEvent,
    ) -> anyhow::Result<StimulateResult> {
        if self.fpaa_available() {
            if let Some(line) = endpoint.gpio_line.as_deref() {
                let width = endpoint
                    .pulse_width_ns
                    .unwrap_or_else(|| event.pulse_width_ns.max(1));
                match self.gpio.emit_pulse(line, width, event.value).await {
                    Ok(()) => {
                        return Ok(StimulateResult::HardwarePulse {
                            line: line.to_string(),
                            pulse_width_ns: width,
                            value: event.value,
                        });
                    }
                    Err(err) => {
                        warn!(
                            line = line,
                            error = %err,
                            "hardware stimulation failed; switching to software fallback"
                        );
                        self.mark_fpaa_unavailable();
                    }
                }
            } else {
                debug!(
                    endpoint_type = ?endpoint.endpoint_type,
                    "FPAA marked available but endpoint has no GPIO line; falling back to software kernel"
                );
            }
        }

        let result = self.software_kernels.execute_endpoint(endpoint, event);
        info!(
            kernel = ?result.kernel,
            synapse_id = %event.synapse_id,
            output_value = result.output_value,
            delay_ns = result.delay_ns,
            "software kernel fulfilled local stimulus"
        );
        Ok(StimulateResult::SoftwareKernel {
            kernel: result.kernel,
            output_value: result.output_value,
            delay_ns: result.delay_ns,
            reason: "fpaa_unavailable".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{PikaHat, StimulateResult};
    use crate::aer::{AerEvent, AerFlags, SynapseId};
    use crate::hardware::gpio_mock::MockGpioBackend;
    use crate::hardware::software_kernel::SoftwareKernel;
    use crate::hardware::spi_mock::MockSpiBackend;
    use crate::routing::endpoint_table::{EndpointRoute, EndpointType};
    use crate::routing::synapse_table::SynapseEndpoint;
    use std::sync::Arc;

    fn endpoint(kernel: Option<SoftwareKernel>) -> SynapseEndpoint {
        SynapseEndpoint {
            endpoint_id: None,
            endpoint_type: EndpointType::DendriteBouton,
            location: None,
            node_slot: 0,
            fpaa_index: Some(0),
            neuron_id: Some(1),
            bouton_id: Some(2),
            io_name: None,
            route: EndpointRoute::LocalSameFpaa,
            weight: Some(1.0),
            delay_ns: None,
            pulse_width_ns: Some(5_000),
            gpio_line: Some("FPAA0_IO5P".to_string()),
            gpio_mask: None,
            software_kernel: kernel,
        }
    }

    fn event() -> AerEvent {
        AerEvent {
            synapse_id: SynapseId(0x5001_0002_0000_1234),
            flags: AerFlags::empty(),
            value: 1,
            event_time_ns: 1,
            pulse_width_ns: 5_000,
            ttl: 8,
            source_node_slot: 0,
            sequence: 1,
        }
    }

    #[tokio::test]
    async fn hardware_pulse_used_when_fpaa_probe_succeeds() {
        let gpio = Arc::new(MockGpioBackend::default());
        let spi = Arc::new(MockSpiBackend::default());
        let hat = PikaHat::new(4, gpio.clone(), spi, false);
        assert!(hat.detect_fpaa().await);
        let result = hat
            .stimulate_endpoint(&endpoint(None), &event())
            .await
            .unwrap();
        assert!(matches!(result, StimulateResult::HardwarePulse { .. }));
        assert_eq!(gpio.pulses().len(), 1);
    }

    #[tokio::test]
    async fn software_kernel_used_when_fpaa_probe_fails() {
        let gpio = Arc::new(MockGpioBackend::default());
        let spi = Arc::new(MockSpiBackend::with_probe_ok(false));
        let hat = PikaHat::new(4, gpio, spi, false);
        assert!(!hat.detect_fpaa().await);
        let result = hat
            .stimulate_endpoint(
                &endpoint(Some(SoftwareKernel::ShortTermPlasticity)),
                &event(),
            )
            .await
            .unwrap();
        assert!(matches!(
            result,
            StimulateResult::SoftwareKernel {
                kernel: SoftwareKernel::ShortTermPlasticity,
                ..
            }
        ));
    }
}
