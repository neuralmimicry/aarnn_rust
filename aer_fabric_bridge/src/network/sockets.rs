use crate::config::NodeNetworkConfig;
use anyhow::{Context, bail};
use socket2::{Domain, Protocol, Socket, Type};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use tokio::net::UdpSocket;

pub fn bind_event_rx_socket(cfg: &NodeNetworkConfig) -> anyhow::Result<UdpSocket> {
    let bind_addr = format!("{}:{}", cfg.bind_ip, cfg.event_port);
    let std_socket = std::net::UdpSocket::bind(&bind_addr)
        .with_context(|| format!("failed to bind event socket on {bind_addr}"))?;
    std_socket
        .set_nonblocking(true)
        .context("failed to set nonblocking on event socket")?;
    Ok(UdpSocket::from_std(std_socket)?)
}

pub fn bind_event_tx_socket(cfg: &NodeNetworkConfig) -> anyhow::Result<UdpSocket> {
    let bind_addr = format!("{}:0", cfg.bind_ip);
    let std_socket = std::net::UdpSocket::bind(&bind_addr)
        .with_context(|| format!("failed to bind event tx socket on {bind_addr}"))?;
    std_socket
        .set_nonblocking(true)
        .context("failed to set nonblocking on event tx socket")?;
    Ok(UdpSocket::from_std(std_socket)?)
}

pub fn bind_control_socket(cfg: &NodeNetworkConfig) -> anyhow::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
        .context("failed to create control socket")?;
    socket
        .set_reuse_address(true)
        .context("failed to set SO_REUSEADDR")?;

    let bind_ip: Ipv4Addr = cfg.bind_ip.parse().unwrap_or(Ipv4Addr::UNSPECIFIED);
    let bind_addr = SocketAddrV4::new(bind_ip, cfg.control_port);
    socket
        .bind(&bind_addr.into())
        .with_context(|| format!("failed to bind control socket on {bind_addr}"))?;

    let multicast = cfg
        .multicast_addr
        .parse::<SocketAddr>()
        .with_context(|| format!("invalid multicast address '{}'", cfg.multicast_addr))?;
    let SocketAddr::V4(multicast_v4) = multicast else {
        bail!("only IPv4 multicast is currently supported");
    };
    socket
        .join_multicast_v4(multicast_v4.ip(), &Ipv4Addr::UNSPECIFIED)
        .with_context(|| format!("failed to join multicast group {}", multicast_v4.ip()))?;
    socket
        .set_multicast_loop_v4(true)
        .context("failed to enable multicast loopback")?;
    socket
        .set_nonblocking(true)
        .context("failed to set nonblocking on control socket")?;

    let std_socket: std::net::UdpSocket = socket.into();
    Ok(UdpSocket::from_std(std_socket)?)
}
