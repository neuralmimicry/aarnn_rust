#[derive(Clone, Debug)]
pub struct AerEvent {
    pub ts_us: u64,
    pub addr: u32,
    pub value: u8,
}

#[derive(Debug)]
pub enum AerError {
    InvalidMagic,
    Truncated,
    VarintOverflow,
}

const MAGIC: &[u8; 4] = b"AER1";

pub fn encode_events(events: &[AerEvent]) -> Vec<u8> {
    if events.is_empty() {
        return Vec::new();
    }

    let mut sorted = events.to_vec();
    sorted.sort_by_key(|ev| ev.ts_us);

    let base_ts = sorted[0].ts_us;
    let mut out = Vec::with_capacity(12 + sorted.len() * 6);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&base_ts.to_le_bytes());

    let mut prev_ts = base_ts;
    for ev in sorted {
        let delta = ev.ts_us.saturating_sub(prev_ts);
        prev_ts = ev.ts_us;
        write_varint(delta, &mut out);
        write_varint(ev.addr as u64, &mut out);
        write_varint(ev.value as u64, &mut out);
    }

    out
}

pub fn decode_events(bytes: &[u8]) -> Result<Vec<AerEvent>, AerError> {
    if bytes.len() < 12 {
        return Err(AerError::Truncated);
    }
    if &bytes[..4] != MAGIC {
        return Err(AerError::InvalidMagic);
    }

    let mut idx = 4;
    let mut base = [0u8; 8];
    base.copy_from_slice(&bytes[idx..idx + 8]);
    idx += 8;
    let base_ts = u64::from_le_bytes(base);
    let mut prev_ts = base_ts;
    let mut events = Vec::new();

    while idx < bytes.len() {
        let (delta, used) = read_varint(&bytes[idx..])?;
        idx += used;
        let (addr, used) = read_varint(&bytes[idx..])?;
        idx += used;
        let (value, used) = read_varint(&bytes[idx..])?;
        idx += used;

        prev_ts = prev_ts.saturating_add(delta);
        events.push(AerEvent {
            ts_us: prev_ts,
            addr: addr as u32,
            value: (value & 0xff) as u8,
        });
    }

    Ok(events)
}

fn addr_to_index(addr: u32, base_addr: u32) -> usize {
    if addr >= base_addr {
        (addr - base_addr) as usize
    } else {
        addr as usize
    }
}

/// Convert a spike vector into AER events using a base address.
pub fn spikes_to_events(ts_us: u64, base_addr: u32, spikes: &[i8]) -> Vec<AerEvent> {
    let mut events = Vec::new();
    for (idx, &spk) in spikes.iter().enumerate() {
        if spk == 0 {
            continue;
        }
        events.push(AerEvent {
            ts_us,
            addr: base_addr + idx as u32,
            value: if spk > 0 { 1 } else { 0 },
        });
    }
    events
}

/// Encode a spike vector into a compact AER payload.
pub fn encode_spikes(ts_us: u64, base_addr: u32, spikes: &[i8]) -> Vec<u8> {
    let events = spikes_to_events(ts_us, base_addr, spikes);
    encode_events(&events)
}

/// Apply AER events to an existing spike vector.
/// Returns the number of spikes set.
pub fn apply_events_to_spikes(events: &[AerEvent], base_addr: u32, dst: &mut [i8]) -> usize {
    let mut count = 0;
    for ev in events {
        if ev.value == 0 {
            continue;
        }
        let idx = addr_to_index(ev.addr, base_addr);
        if idx < dst.len() {
            dst[idx] = 1;
            count += 1;
        }
    }
    count
}

/// Decode AER payload into the provided spike vector.
/// Returns the number of spikes set.
pub fn decode_spikes(bytes: &[u8], base_addr: u32, dst: &mut [i8]) -> Result<usize, AerError> {
    let events = decode_events(bytes)?;
    Ok(apply_events_to_spikes(&events, base_addr, dst))
}

/// Decode AER payload into a freshly allocated spike vector of length `len`.
pub fn decode_spikes_vec(bytes: &[u8], base_addr: u32, len: usize) -> Result<Vec<i8>, AerError> {
    let mut out = vec![0i8; len];
    let _ = decode_spikes(bytes, base_addr, &mut out)?;
    Ok(out)
}

fn write_varint(mut value: u64, out: &mut Vec<u8>) {
    while value >= 0x80 {
        out.push(((value as u8) & 0x7f) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn read_varint(bytes: &[u8]) -> Result<(u64, usize), AerError> {
    let mut result = 0u64;
    let mut shift = 0u32;
    for (i, &b) in bytes.iter().enumerate() {
        let val = (b & 0x7f) as u64;
        result |= val << shift;
        if b & 0x80 == 0 {
            return Ok((result, i + 1));
        }
        shift += 7;
        if shift >= 64 {
            return Err(AerError::VarintOverflow);
        }
    }
    Err(AerError::Truncated)
}
