use crate::hardware::gpio::{GpioBackend, GpioEdge};
use async_trait::async_trait;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::Arc;
#[cfg(feature = "linux-gpio")]
use std::time::Duration;
#[cfg(feature = "linux-gpio")]
use tracing::{debug, info, warn};

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct LineBinding {
    chip_path: String,
    offset: u32,
}

pub struct LinuxGpioBackend {
    #[cfg(feature = "linux-gpio")]
    default_chip: String,
    #[cfg(feature = "linux-gpio")]
    consumer: String,
    #[cfg(feature = "linux-gpio")]
    alias_map: HashMap<String, String>,
    #[cfg(feature = "linux-gpio")]
    resolved: Mutex<HashMap<String, LineBinding>>,
    edge_queue: Arc<Mutex<VecDeque<GpioEdge>>>,
    #[cfg(feature = "linux-gpio")]
    outputs: Mutex<HashMap<String, Arc<Mutex<gpiod::Lines<gpiod::Output>>>>>,
}

impl LinuxGpioBackend {
    pub fn new(
        default_chip: impl Into<String>,
        consumer: impl Into<String>,
        alias_map: HashMap<String, String>,
        capture_lines: Vec<String>,
    ) -> anyhow::Result<Self> {
        #[cfg(not(feature = "linux-gpio"))]
        let _ = (&default_chip, &consumer, &alias_map, &capture_lines);

        let backend = Self {
            #[cfg(feature = "linux-gpio")]
            default_chip: normalise_chip_path(default_chip.into()),
            #[cfg(feature = "linux-gpio")]
            consumer: consumer.into(),
            #[cfg(feature = "linux-gpio")]
            alias_map,
            #[cfg(feature = "linux-gpio")]
            resolved: Mutex::new(HashMap::new()),
            edge_queue: Arc::new(Mutex::new(VecDeque::with_capacity(256))),
            #[cfg(feature = "linux-gpio")]
            outputs: Mutex::new(HashMap::new()),
        };
        #[cfg(feature = "linux-gpio")]
        backend.start_capture_monitors(capture_lines);
        Ok(backend)
    }

    #[cfg(feature = "linux-gpio")]
    fn normalise_line_alias<'a>(&'a self, line: &'a str) -> &'a str {
        self.alias_map.get(line).map(String::as_str).unwrap_or(line)
    }

    #[cfg(feature = "linux-gpio")]
    fn resolve_line(&self, line: &str) -> anyhow::Result<LineBinding> {
        if let Some(existing) = self.resolved.lock().get(line).cloned() {
            return Ok(existing);
        }

        let alias = self.normalise_line_alias(line);
        let binding = if let Some((chip_path, offset)) = parse_line_spec(alias, &self.default_chip)
        {
            LineBinding { chip_path, offset }
        } else {
            self.find_line_by_name(alias)?
        };
        self.resolved
            .lock()
            .insert(line.to_string(), binding.clone());
        Ok(binding)
    }

    #[cfg(feature = "linux-gpio")]
    fn find_line_by_name(&self, target_name: &str) -> anyhow::Result<LineBinding> {
        let chips = gpiod::Chip::list_devices()
            .map_err(|err| anyhow::anyhow!("failed to enumerate GPIO chips: {err}"))?;
        for chip_path in chips {
            let chip = match gpiod::Chip::new(&chip_path) {
                Ok(chip) => chip,
                Err(err) => {
                    debug!(chip = %chip_path.display(), error = %err, "skipping unreadable GPIO chip");
                    continue;
                }
            };
            for offset in 0..chip.num_lines() {
                let Ok(info) = chip.line_info(offset) else {
                    continue;
                };
                if info.name == target_name {
                    return Ok(LineBinding {
                        chip_path: chip_path.display().to_string(),
                        offset,
                    });
                }
            }
        }
        anyhow::bail!("unable to resolve GPIO line '{}'", target_name);
    }

    #[cfg(feature = "linux-gpio")]
    fn output_handle(
        &self,
        line: &str,
        binding: &LineBinding,
    ) -> anyhow::Result<Arc<Mutex<gpiod::Lines<gpiod::Output>>>> {
        if let Some(existing) = self.outputs.lock().get(line).cloned() {
            return Ok(existing);
        }

        let chip = gpiod::Chip::new(&binding.chip_path).map_err(|err| {
            anyhow::anyhow!(
                "failed to open GPIO chip '{}' for '{}': {}",
                binding.chip_path,
                line,
                err
            )
        })?;
        let opts = gpiod::Options::output([binding.offset])
            .values([false])
            .consumer(&self.consumer);
        let output = chip.request_lines(opts).map_err(|err| {
            anyhow::anyhow!(
                "failed to request output line '{}' (chip={} offset={}): {}",
                line,
                binding.chip_path,
                binding.offset,
                err
            )
        })?;
        let output = Arc::new(Mutex::new(output));
        self.outputs
            .lock()
            .insert(line.to_string(), Arc::clone(&output));
        Ok(output)
    }

    #[cfg(feature = "linux-gpio")]
    fn start_capture_monitors(&self, capture_lines: Vec<String>) {
        if capture_lines.is_empty() {
            return;
        }

        let mut by_chip: HashMap<String, Vec<(u32, String)>> = HashMap::new();
        for logical_line in capture_lines {
            match self.resolve_line(&logical_line) {
                Ok(binding) => {
                    by_chip
                        .entry(binding.chip_path)
                        .or_default()
                        .push((binding.offset, logical_line));
                }
                Err(err) => {
                    warn!(line = logical_line, error = %err, "capture line will be ignored");
                }
            }
        }

        for (chip_path, mut bindings) in by_chip {
            bindings.sort_by_key(|(offset, _)| *offset);
            bindings.dedup_by_key(|(offset, _)| *offset);
            let offsets: Vec<u32> = bindings.iter().map(|(offset, _)| *offset).collect();
            let names_by_bit: Vec<String> = bindings.into_iter().map(|(_, name)| name).collect();
            let queue = Arc::clone(&self.edge_queue);
            let consumer = format!("{}-capture", self.consumer);
            let chip_path_for_spawn = chip_path.clone();
            let thread_name = format!(
                "aer-gpio-{}",
                chip_path
                    .rsplit('/')
                    .next()
                    .unwrap_or("capture")
                    .to_string()
            );

            let builder = std::thread::Builder::new().name(thread_name);
            let spawn_result = builder.spawn(move || {
                let chip = match gpiod::Chip::new(&chip_path_for_spawn) {
                    Ok(chip) => chip,
                    Err(err) => {
                        warn!(chip = %chip_path_for_spawn, error = %err, "failed to open capture chip");
                        return;
                    }
                };
                let opts = gpiod::Options::input(offsets)
                    .edge(gpiod::EdgeDetect::Both)
                    .consumer(consumer);
                let mut inputs = match chip.request_lines(opts) {
                    Ok(lines) => lines,
                    Err(err) => {
                        warn!(chip = %chip_path_for_spawn, error = %err, "failed to request capture lines");
                        return;
                    }
                };

                info!(chip = %chip_path_for_spawn, "started GPIO edge capture monitor");
                loop {
                    match inputs.read_event() {
                        Ok(event) => {
                            let line = names_by_bit
                                .get(event.line as usize)
                                .cloned()
                                .unwrap_or_else(|| format!("line{}", event.line));
                            let timestamp_ns = event.time.as_nanos().min(u64::MAX as u128) as u64;
                            let rising = matches!(event.edge, gpiod::Edge::Rising);
                            let mut queue_guard = queue.lock();
                            if queue_guard.len() >= 8_192 {
                                let _ = queue_guard.pop_front();
                            }
                            queue_guard.push_back(GpioEdge {
                                line,
                                rising,
                                timestamp_ns,
                            });
                        }
                        Err(err) => {
                            warn!(chip = %chip_path_for_spawn, error = %err, "GPIO edge monitor read failed");
                            std::thread::sleep(Duration::from_millis(20));
                        }
                    }
                }
            });

            if let Err(err) = spawn_result {
                warn!(chip = %chip_path, error = %err, "failed to spawn GPIO monitor thread");
            }
        }
    }

    #[cfg(not(feature = "linux-gpio"))]
    #[allow(dead_code)]
    fn start_capture_monitors(&self, _capture_lines: Vec<String>) {}
}

#[async_trait]
impl GpioBackend for LinuxGpioBackend {
    async fn emit_pulse(&self, line: &str, pulse_width_ns: u32, value: u32) -> anyhow::Result<()> {
        #[cfg(feature = "linux-gpio")]
        {
            let binding = self.resolve_line(line)?;
            let output = self.output_handle(line, &binding)?;
            let high = value != 0;
            {
                let guard = output.lock();
                guard.set_values([high]).map_err(|err| {
                    anyhow::anyhow!(
                        "failed to set high value on line '{}' (chip={} offset={}): {}",
                        line,
                        binding.chip_path,
                        binding.offset,
                        err
                    )
                })?;
            }

            tokio::time::sleep(Duration::from_nanos((pulse_width_ns as u64).max(1))).await;
            {
                let guard = output.lock();
                guard.set_values([false]).map_err(|err| {
                    anyhow::anyhow!(
                        "failed to reset line '{}' low (chip={} offset={}): {}",
                        line,
                        binding.chip_path,
                        binding.offset,
                        err
                    )
                })?;
            }
            Ok(())
        }
        #[cfg(not(feature = "linux-gpio"))]
        {
            let _ = (line, pulse_width_ns, value);
            anyhow::bail!("linux-gpio backend requested without linux-gpio feature")
        }
    }

    async fn read_edges(&self) -> anyhow::Result<Vec<GpioEdge>> {
        let mut edges = self.edge_queue.lock();
        Ok(edges.drain(..).collect())
    }
}

#[cfg(feature = "linux-gpio")]
fn normalise_chip_path(raw: String) -> String {
    if raw.starts_with("/dev/") {
        raw
    } else {
        format!("/dev/{}", raw)
    }
}

#[cfg(feature = "linux-gpio")]
fn parse_line_spec(raw: &str, default_chip: &str) -> Option<(String, u32)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(offset) = trimmed.parse::<u32>() {
        return Some((normalise_chip_path(default_chip.to_string()), offset));
    }
    if let Some((chip, offset_raw)) = trimmed.rsplit_once(':')
        && let Ok(offset) = offset_raw.parse::<u32>()
    {
        let chip = if chip.trim().is_empty() {
            default_chip.to_string()
        } else {
            chip.trim().to_string()
        };
        return Some((normalise_chip_path(chip), offset));
    }
    None
}
