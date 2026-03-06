use crate::aer::{decode_events, encode_events, AerEvent};
use std::collections::VecDeque;
use std::io;
use std::net::{SocketAddr, UdpSocket};

#[derive(Clone, Debug)]
pub struct AerIoConfig {
    pub listen_addr: Option<String>,
    pub peer_addr: Option<String>,
    pub sensory_base: u32,
    pub output_base: u32,
    pub max_events: usize,
    pub max_packet_bytes: usize,
}

impl Default for AerIoConfig {
    fn default() -> Self {
        Self {
            listen_addr: None,
            peer_addr: None,
            sensory_base: 0x1000,
            output_base: 0x4000,
            max_events: 4096,
            max_packet_bytes: 8192,
        }
    }
}

impl AerIoConfig {
    pub fn enabled(&self) -> bool {
        self.listen_addr.is_some() || self.peer_addr.is_some()
    }
}

pub struct AerLink {
    socket: UdpSocket,
    peer: Option<SocketAddr>,
    buffer: VecDeque<AerEvent>,
    cfg: AerIoConfig,
}

impl AerLink {
    pub fn bind(cfg: AerIoConfig) -> io::Result<Self> {
        let bind_addr = cfg
            .listen_addr
            .as_deref()
            .unwrap_or("0.0.0.0:0")
            .parse::<SocketAddr>()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let socket = UdpSocket::bind(bind_addr)?;
        socket.set_nonblocking(true)?;
        let peer = match cfg.peer_addr.as_deref() {
            Some(addr) => Some(
                addr.parse::<SocketAddr>()
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?,
            ),
            None => None,
        };
        Ok(Self {
            socket,
            peer,
            buffer: VecDeque::new(),
            cfg,
        })
    }

    pub fn poll(&mut self) {
        let mut buf = vec![0u8; self.cfg.max_packet_bytes];
        loop {
            match self.socket.recv_from(&mut buf) {
                Ok((size, _)) => {
                    if let Ok(events) = decode_events(&buf[..size]) {
                        self.push_events(events);
                    }
                }
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }
    }

    pub fn sensory_spikes(&mut self, start_us: u64, end_us: u64, n_sensory: usize) -> Vec<i8> {
        let mut spikes = vec![0i8; n_sensory];
        let mut remaining = VecDeque::new();
        while let Some(ev) = self.buffer.pop_front() {
            if ev.ts_us < start_us {
                continue;
            }
            if ev.ts_us <= end_us {
                if ev.value == 0 {
                    continue;
                }
                if ev.addr >= self.cfg.sensory_base {
                    let idx = (ev.addr - self.cfg.sensory_base) as usize;
                    if idx < n_sensory {
                        spikes[idx] = 1;
                    }
                }
            } else {
                remaining.push_back(ev);
            }
        }
        self.buffer = remaining;
        spikes
    }

    pub fn send_output_spikes(&mut self, ts_us: u64, spikes: &[i8]) {
        let peer = match self.peer {
            Some(addr) => addr,
            None => return,
        };
        let mut events = Vec::new();
        for (idx, &spk) in spikes.iter().enumerate() {
            if spk == 0 {
                continue;
            }
            events.push(AerEvent {
                ts_us,
                addr: self.cfg.output_base + idx as u32,
                value: if spk > 0 { 1 } else { 0 },
            });
        }
        if events.is_empty() {
            return;
        }
        let payload = encode_events(&events);
        let _ = self.socket.send_to(&payload, peer);
    }

    fn push_events(&mut self, events: Vec<AerEvent>) {
        for ev in events {
            self.buffer.push_back(ev);
        }
        while self.buffer.len() > self.cfg.max_events {
            self.buffer.pop_front();
        }
    }
}
