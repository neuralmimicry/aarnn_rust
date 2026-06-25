use crate::hardware::spi::SpiBackend;
use async_trait::async_trait;
#[cfg(feature = "linux-spi")]
use parking_lot::Mutex;
#[cfg(feature = "linux-spi")]
use spidev::{SpiModeFlags, Spidev, SpidevOptions, SpidevTransfer};
#[cfg(feature = "linux-spi")]
use std::io::Write;
#[cfg(feature = "linux-spi")]
use std::sync::Arc;
#[cfg(feature = "linux-spi")]
use tracing::{debug, warn};

#[derive(Debug, Clone)]
pub struct LinuxSpiConfig {
    pub device: String,
    pub speed_hz: u32,
    pub bits_per_word: u8,
    pub mode: u8,
    pub lsb_first: bool,
    pub probe_bytes: Vec<u8>,
}

impl Default for LinuxSpiConfig {
    fn default() -> Self {
        Self {
            device: "/dev/spidev0.0".to_string(),
            speed_hz: 2_000_000,
            bits_per_word: 8,
            mode: 0,
            lsb_first: false,
            probe_bytes: vec![0xAA, 0x55],
        }
    }
}

pub struct LinuxSpiBackend {
    config: LinuxSpiConfig,
    init_error: Option<String>,
    #[cfg(feature = "linux-spi")]
    inner: Option<Arc<Mutex<Spidev>>>,
}

impl LinuxSpiBackend {
    pub fn new(config: LinuxSpiConfig) -> Self {
        #[cfg(feature = "linux-spi")]
        {
            match open_spidev(&config) {
                Ok(spi) => Self {
                    config,
                    init_error: None,
                    inner: Some(Arc::new(Mutex::new(spi))),
                },
                Err(err) => {
                    warn!(
                        device = config.device,
                        error = %err,
                        "linux SPI backend initialisation failed; software fallback will be used"
                    );
                    Self {
                        config,
                        init_error: Some(err.to_string()),
                        inner: None,
                    }
                }
            }
        }
        #[cfg(not(feature = "linux-spi"))]
        {
            Self {
                config,
                init_error: Some("linux-spi feature not enabled".to_string()),
            }
        }
    }

    fn unavailable_error(&self) -> anyhow::Error {
        anyhow::anyhow!(
            "linux spi backend unavailable for '{}': {}",
            self.config.device,
            self.init_error
                .as_deref()
                .unwrap_or("unknown initialisation error")
        )
    }
}

#[cfg(feature = "linux-spi")]
fn open_spidev(config: &LinuxSpiConfig) -> anyhow::Result<Spidev> {
    let mut spi = Spidev::open(&config.device)
        .map_err(|err| anyhow::anyhow!("failed to open SPI device '{}': {}", config.device, err))?;
    let mode_flags = match config.mode & 0x3 {
        0 => SpiModeFlags::SPI_MODE_0,
        1 => SpiModeFlags::SPI_MODE_1,
        2 => SpiModeFlags::SPI_MODE_2,
        _ => SpiModeFlags::SPI_MODE_3,
    };
    let opts = SpidevOptions::new()
        .bits_per_word(config.bits_per_word.max(1))
        .max_speed_hz(config.speed_hz.max(100_000))
        .lsb_first(config.lsb_first)
        .mode(mode_flags)
        .build();
    spi.configure(&opts).map_err(|err| {
        anyhow::anyhow!(
            "failed to configure SPI device '{}' (speed={}Hz bits={} mode={}): {}",
            config.device,
            config.speed_hz,
            config.bits_per_word,
            config.mode,
            err
        )
    })?;
    Ok(spi)
}

#[async_trait]
impl SpiBackend for LinuxSpiBackend {
    async fn transfer(&self, bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
        #[cfg(feature = "linux-spi")]
        {
            let Some(inner) = self.inner.as_ref() else {
                return Err(self.unavailable_error());
            };
            let mut rx = vec![0u8; bytes.len()];
            {
                let spi = inner.lock();
                let mut transfer = SpidevTransfer::read_write(bytes, &mut rx);
                spi.transfer(&mut transfer).map_err(|err| {
                    anyhow::anyhow!(
                        "SPI transfer failed on '{}' ({} bytes): {}",
                        self.config.device,
                        bytes.len(),
                        err
                    )
                })?;
            }
            debug!(
                device = self.config.device,
                tx_bytes = bytes.len(),
                "linux SPI transfer completed"
            );
            Ok(rx)
        }
        #[cfg(not(feature = "linux-spi"))]
        {
            let _ = bytes;
            Err(self.unavailable_error())
        }
    }

    async fn write(&self, bytes: &[u8]) -> anyhow::Result<()> {
        #[cfg(feature = "linux-spi")]
        {
            let Some(inner) = self.inner.as_ref() else {
                return Err(self.unavailable_error());
            };
            {
                let mut spi = inner.lock();
                spi.write_all(bytes).map_err(|err| {
                    anyhow::anyhow!(
                        "SPI write failed on '{}' ({} bytes): {}",
                        self.config.device,
                        bytes.len(),
                        err
                    )
                })?;
            }
            debug!(
                device = self.config.device,
                bytes = bytes.len(),
                "linux SPI write completed"
            );
            Ok(())
        }
        #[cfg(not(feature = "linux-spi"))]
        {
            let _ = bytes;
            Err(self.unavailable_error())
        }
    }

    async fn probe_device(&self) -> anyhow::Result<()> {
        let probe = if self.config.probe_bytes.is_empty() {
            vec![0x00]
        } else {
            self.config.probe_bytes.clone()
        };
        let _ = self.transfer(&probe).await?;
        Ok(())
    }
}
