use std::time::Duration;

use clap::Parser;
use tokio::sync::watch;

#[path = "../tcp_aer_ipc_bridge.rs"]
mod tcp_aer_ipc_bridge;

use tcp_aer_ipc_bridge::BridgeConfig;

#[derive(Debug, Parser)]
#[command(about = "Bridge length-prefixed simulator TCP/AER1 traffic to AARNN IPC")]
struct Args {
    /// TCP listener in HOST:PORT form.
    #[arg(long, value_parser = parse_listen)]
    listen: ListenAddress,
    /// Distributed brain Unix datagram socket.
    #[arg(long)]
    ipc: std::path::PathBuf,
    #[arg(long, default_value_t = 1)]
    sensory: usize,
    #[arg(long, default_value_t = 1)]
    output: usize,
    #[arg(long, default_value_t = 16_384)]
    aer_output_base: u32,
    #[arg(long, default_value_t = 0.5)]
    aer_threshold: f32,
    #[arg(long, default_value_t = 300.0, value_parser = parse_timeout)]
    timeout: f64,
    #[arg(long)]
    ready_file: Option<std::path::PathBuf>,
    #[arg(long)]
    arm_file: Option<std::path::PathBuf>,
}

#[derive(Clone, Debug)]
struct ListenAddress {
    host: String,
    port: u16,
}

fn parse_listen(value: &str) -> Result<ListenAddress, String> {
    let (host, port) = if let Some(rest) = value.strip_prefix('[') {
        let (host, port) = rest
            .split_once("]:")
            .ok_or_else(|| format!("expected HOST:PORT, got {value:?}"))?;
        (host.to_string(), port)
    } else {
        let (host, port) = value
            .rsplit_once(':')
            .ok_or_else(|| format!("expected HOST:PORT, got {value:?}"))?;
        (host.to_string(), port)
    };
    if host.is_empty() {
        return Err(format!("listener host is empty in {value:?}"));
    }
    let port = port
        .parse::<u16>()
        .map_err(|_| format!("invalid listener port in {value:?}"))?;
    if port == 0 {
        return Err("listener port must be in 1..=65535".into());
    }
    Ok(ListenAddress { host, port })
}

fn parse_timeout(value: &str) -> Result<f64, String> {
    let timeout = value
        .parse::<f64>()
        .map_err(|_| format!("invalid timeout {value:?}"))?;
    if !timeout.is_finite() || timeout <= 0.0 {
        return Err("timeout must be finite and greater than zero".into());
    }
    Ok(timeout)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let signal_tx = shutdown_tx.clone();
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            let mut terminate =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("install SIGTERM handler");
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {},
                _ = terminate.recv() => {},
            }
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
        }
        let _ = signal_tx.send(true);
    });

    let config = BridgeConfig {
        listen_host: args.listen.host,
        listen_port: args.listen.port,
        ipc_path: args.ipc,
        sensory: args.sensory,
        output: args.output,
        aer_output_base: args.aer_output_base,
        aer_threshold: args.aer_threshold,
        timeout: Duration::from_secs_f64(args.timeout),
        ready_file: args.ready_file,
        arm_file: args.arm_file,
    };
    tcp_aer_ipc_bridge::run(config, shutdown_rx).await?;
    Ok(())
}
