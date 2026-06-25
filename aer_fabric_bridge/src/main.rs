use aer_fabric_bridge::aer::{AerEvent, AerFlags, SynapseId, encode_packet};
use aer_fabric_bridge::config::ConfigBundle;
use aer_fabric_bridge::runtime::tasks::run_bridge;
use aer_fabric_bridge::time::unix_time_ns;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::str::FromStr;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "aer-fabric-bridge")]
#[command(about = "Synapse-addressed AER bridge for Pi4 FPAA clusters")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Run {
        #[arg(long)]
        config: Option<PathBuf>,
    },
    ValidateConfig {
        #[arg(long)]
        config: Option<PathBuf>,
    },
    SendTestEvent {
        #[arg(long)]
        synapse: String,
        #[arg(long)]
        target: String,
        #[arg(long, default_value_t = 1)]
        value: u32,
        #[arg(long, default_value_t = 8)]
        ttl: u8,
        #[arg(long, default_value_t = 5_000)]
        pulse_width_ns: u32,
        #[arg(long, default_value_t = 0)]
        source_slot: u16,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    let cli = Cli::parse();

    match cli.command {
        Command::Run { config } => {
            let bundle = ConfigBundle::load(config.as_deref())?;
            run_bridge(bundle).await?;
        }
        Command::ValidateConfig { config } => {
            let bundle = ConfigBundle::load(config.as_deref())?;
            println!(
                "config valid: node={} uuid={} cluster={} synapses={} host_subscribers={}",
                bundle.node.node_name,
                bundle.node.node_uuid,
                bundle.node.cluster_name,
                bundle.synapses.all().count(),
                bundle.cluster.host_subscribers.len()
            );
        }
        Command::SendTestEvent {
            synapse,
            target,
            value,
            ttl,
            pulse_width_ns,
            source_slot,
        } => {
            send_test_event(&synapse, &target, value, ttl, pulse_width_ns, source_slot)?;
        }
    }

    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

fn send_test_event(
    synapse_raw: &str,
    target_raw: &str,
    value: u32,
    ttl: u8,
    pulse_width_ns: u32,
    source_slot: u16,
) -> anyhow::Result<()> {
    let synapse_id = SynapseId::from_str(synapse_raw).map_err(anyhow::Error::msg)?;
    let target = target_raw.parse::<std::net::SocketAddr>()?;
    let event = AerEvent {
        synapse_id,
        flags: AerFlags::empty(),
        value,
        event_time_ns: unix_time_ns(),
        pulse_width_ns,
        ttl,
        source_node_slot: source_slot,
        sequence: 1,
    };
    let payload = encode_packet(source_slot, 1, unix_time_ns(), &[event])?;
    let socket = std::net::UdpSocket::bind("0.0.0.0:0")?;
    socket.send_to(&payload, target)?;
    println!(
        "sent test event synapse={} target={} value={} ttl={}",
        synapse_id, target, value, ttl
    );
    Ok(())
}
