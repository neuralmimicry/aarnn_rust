use crate::routing::{LocalSynapseTable, SynapseRange};
use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use uuid::Uuid;

const DEFAULT_NODE_CONFIG_PATH: &str = "/etc/aer-bridge/node.toml";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    pub node_uuid: Uuid,
    pub node_name: String,
    pub cluster_name: String,
    #[serde(default)]
    pub network: NodeNetworkConfig,
    #[serde(default)]
    pub hardware: NodeHardwareConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NodeConfigFile {
    pub node_uuid: Option<Uuid>,
    #[serde(default = "default_node_name")]
    pub node_name: String,
    #[serde(default = "default_cluster_name")]
    pub cluster_name: String,
    #[serde(default)]
    pub network: NodeNetworkConfig,
    #[serde(default)]
    pub hardware: NodeHardwareConfig,
}

fn default_node_name() -> String {
    "aer-pika-bridge".to_string()
}

fn default_cluster_name() -> String {
    "aarnn-fpaa-cluster".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeNetworkConfig {
    #[serde(default = "default_interface")]
    pub interface: String,
    #[serde(default)]
    pub wifi_interface: Option<String>,
    #[serde(default = "default_bind_ip")]
    pub bind_ip: String,
    #[serde(default)]
    pub advertise_ip: Option<String>,
    #[serde(default = "default_control_port")]
    pub control_port: u16,
    #[serde(default = "default_event_port")]
    pub event_port: u16,
    #[serde(default = "default_diag_port")]
    pub diagnostics_port: u16,
    #[serde(default = "default_multicast_addr")]
    pub multicast_addr: String,
}

fn default_interface() -> String {
    "eth0".to_string()
}
fn default_bind_ip() -> String {
    "0.0.0.0".to_string()
}
fn default_control_port() -> u16 {
    45_880
}
fn default_event_port() -> u16 {
    45_881
}
fn default_diag_port() -> u16 {
    45_882
}
fn default_multicast_addr() -> String {
    "239.192.44.44:45880".to_string()
}

impl Default for NodeNetworkConfig {
    fn default() -> Self {
        Self {
            interface: default_interface(),
            wifi_interface: None,
            bind_ip: default_bind_ip(),
            advertise_ip: None,
            control_port: default_control_port(),
            event_port: default_event_port(),
            diagnostics_port: default_diag_port(),
            multicast_addr: default_multicast_addr(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeHardwareConfig {
    #[serde(default = "default_hat_type")]
    pub hat_type: String,
    #[serde(default = "default_chip_type")]
    pub chip_type: String,
    #[serde(default = "default_fpaa_count")]
    pub fpaa_count: u8,
    #[serde(default = "default_gpio_backend")]
    pub gpio_backend: String,
    #[serde(default = "default_spi_backend")]
    pub spi_backend: String,
    #[serde(default = "default_gpio_chip")]
    pub gpio_chip: String,
    #[serde(default = "default_gpio_consumer")]
    pub gpio_consumer: String,
    #[serde(default = "default_spi_device")]
    pub spi_device: String,
    #[serde(default = "default_spi_speed_hz")]
    pub spi_speed_hz: u32,
    #[serde(default = "default_spi_bits_per_word")]
    pub spi_bits_per_word: u8,
    #[serde(default = "default_spi_mode")]
    pub spi_mode: u8,
    #[serde(default = "default_fpaa_probe_hex")]
    pub fpaa_probe_hex: String,
    #[serde(default)]
    pub force_software_fallback: bool,
}

fn default_hat_type() -> String {
    "okika-pika".to_string()
}
fn default_chip_type() -> String {
    "AN231E04".to_string()
}
fn default_fpaa_count() -> u8 {
    4
}
fn default_gpio_backend() -> String {
    "mock".to_string()
}
fn default_spi_backend() -> String {
    "mock".to_string()
}
fn default_gpio_chip() -> String {
    "gpiochip0".to_string()
}
fn default_gpio_consumer() -> String {
    "aer-fabric-bridge".to_string()
}
fn default_spi_device() -> String {
    "/dev/spidev0.0".to_string()
}
fn default_spi_speed_hz() -> u32 {
    2_000_000
}
fn default_spi_bits_per_word() -> u8 {
    8
}
fn default_spi_mode() -> u8 {
    0
}
fn default_fpaa_probe_hex() -> String {
    "aa55".to_string()
}

impl Default for NodeHardwareConfig {
    fn default() -> Self {
        Self {
            hat_type: default_hat_type(),
            chip_type: default_chip_type(),
            fpaa_count: default_fpaa_count(),
            gpio_backend: default_gpio_backend(),
            spi_backend: default_spi_backend(),
            gpio_chip: default_gpio_chip(),
            gpio_consumer: default_gpio_consumer(),
            spi_device: default_spi_device(),
            spi_speed_hz: default_spi_speed_hz(),
            spi_bits_per_word: default_spi_bits_per_word(),
            spi_mode: default_spi_mode(),
            fpaa_probe_hex: default_fpaa_probe_hex(),
            force_software_fallback: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClusterConfig {
    #[serde(default)]
    pub host_subscribers: Vec<String>,
    #[serde(default)]
    pub owned_synapse_ranges: Vec<SynapseRange>,
    #[serde(default = "default_system_only")]
    pub system_network_only: bool,
}

fn default_system_only() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FpaaMappingConfig {
    #[serde(flatten)]
    pub fpaas: HashMap<String, FpaaIoMapping>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FpaaIoMapping {
    #[serde(default)]
    pub io5p: String,
    #[serde(default)]
    pub io5n: String,
}

impl FpaaMappingConfig {
    pub fn gpio_alias_map(&self) -> HashMap<String, String> {
        let mut aliases = HashMap::new();
        for (key, mapping) in &self.fpaas {
            let Some(index_raw) = key.strip_prefix("fpaa") else {
                continue;
            };
            let Ok(index) = index_raw.parse::<u8>() else {
                continue;
            };
            if !mapping.io5p.trim().is_empty() {
                aliases.insert(format!("FPAA{}_IO5P", index), mapping.io5p.clone());
            }
            if !mapping.io5n.trim().is_empty() {
                aliases.insert(format!("FPAA{}_IO5N", index), mapping.io5n.clone());
            }
        }
        aliases
    }
}

#[derive(Debug)]
pub struct ConfigBundle {
    pub node: NodeConfig,
    pub cluster: ClusterConfig,
    pub synapses: LocalSynapseTable,
    pub fpaa_mapping: FpaaMappingConfig,
    pub node_config_path: PathBuf,
    pub slot_registry_path: PathBuf,
}

impl ConfigBundle {
    pub fn load(node_path: Option<&Path>) -> anyhow::Result<Self> {
        let node_config_path = resolve_node_config_path(node_path)?;
        let node = load_node_config(&node_config_path)?;
        validate_node_config(&node)?;
        let config_dir = node_config_path.parent().unwrap_or(Path::new("."));

        let cluster = load_optional_toml::<ClusterConfig>(&config_dir.join("cluster.toml"))?;
        let synapses = LocalSynapseTable::load_from_file(&config_dir.join("synapses.toml"))?;
        let fpaa_mapping =
            load_optional_toml::<FpaaMappingConfig>(&config_dir.join("fpaa_mapping.toml"))?;
        let slot_registry_path = config_dir.join("node_slots.toml");

        Ok(Self {
            node,
            cluster,
            synapses,
            fpaa_mapping,
            node_config_path,
            slot_registry_path,
        })
    }
}

fn load_optional_toml<T>(path: &Path) -> anyhow::Result<T>
where
    T: Default + for<'de> Deserialize<'de>,
{
    if !path.exists() {
        return Ok(T::default());
    }
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read '{}'", path.display()))?;
    let parsed =
        toml::from_str(&raw).with_context(|| format!("failed to parse '{}'", path.display()))?;
    Ok(parsed)
}

fn resolve_node_config_path(explicit: Option<&Path>) -> anyhow::Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path.to_path_buf());
    }
    let local = PathBuf::from("./config/node.toml");
    if local.exists() {
        return Ok(local);
    }
    let system = PathBuf::from(DEFAULT_NODE_CONFIG_PATH);
    if system.exists() {
        return Ok(system);
    }
    bail!(
        "no node config found; checked '{}' and '{}'",
        local.display(),
        system.display()
    );
}

pub fn load_node_config(path: &Path) -> anyhow::Result<NodeConfig> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read '{}'", path.display()))?;
    let mut parsed: NodeConfigFile =
        toml::from_str(&raw).with_context(|| format!("failed to parse '{}'", path.display()))?;
    let node_uuid = match parsed.node_uuid {
        Some(existing) => existing,
        None => {
            let generated = Uuid::new_v4();
            parsed.node_uuid = Some(generated);
            let rendered = toml::to_string_pretty(&parsed)
                .context("failed to render node config with generated UUID")?;
            std::fs::write(path, rendered).with_context(|| {
                format!(
                    "failed to write node config with generated UUID to '{}'",
                    path.display()
                )
            })?;
            generated
        }
    };
    Ok(NodeConfig {
        node_uuid,
        node_name: parsed.node_name,
        cluster_name: parsed.cluster_name,
        network: parsed.network,
        hardware: parsed.hardware,
    })
}

pub fn validate_node_config(cfg: &NodeConfig) -> anyhow::Result<()> {
    if cfg.node_name.trim().is_empty() {
        bail!("node_name must not be empty");
    }
    if cfg.cluster_name.trim().is_empty() {
        bail!("cluster_name must not be empty");
    }
    if cfg.network.control_port == cfg.network.event_port {
        bail!("control_port and event_port must be different");
    }
    if cfg.hardware.fpaa_count == 0 {
        bail!("fpaa_count must be at least 1");
    }
    if cfg.hardware.spi_bits_per_word == 0 {
        bail!("spi_bits_per_word must be at least 1");
    }
    if cfg.hardware.spi_mode > 3 {
        bail!("spi_mode must be 0..=3");
    }
    if cfg.hardware.spi_speed_hz == 0 {
        bail!("spi_speed_hz must be greater than 0");
    }
    let _ = std::net::SocketAddr::from_str(&cfg.network.multicast_addr)
        .with_context(|| format!("invalid multicast address '{}'", cfg.network.multicast_addr))?;
    Ok(())
}
