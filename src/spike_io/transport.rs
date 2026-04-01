//! Shared spike transport helpers for AER payloads, index lists, and hex framing.

use thiserror::Error;

use crate::aer;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SpikeTransportError {
    #[error("invalid AER payload")]
    InvalidAerPayload,
    #[error("invalid AER hex payload: {0}")]
    InvalidHex(String),
}

/// Compact exchange form used by HTTP/gRPC/UI glue code.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpikeExchange {
    pub aer_payload: Vec<u8>,
    pub aer_base: u32,
    pub spike_indices: Vec<u32>,
}

/// Decode a hex-encoded AER payload, ignoring whitespace and an optional `0x` prefix.
pub fn decode_hex_payload(raw: &str) -> Result<Vec<u8>, SpikeTransportError> {
    let compact: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
    let compact = compact
        .strip_prefix("0x")
        .or_else(|| compact.strip_prefix("0X"))
        .unwrap_or(&compact);
    if compact.is_empty() {
        return Ok(Vec::new());
    }
    hex::decode(compact).map_err(|e| SpikeTransportError::InvalidHex(e.to_string()))
}

/// Apply an AER payload encoded as hex to an existing spike vector.
pub fn apply_hex_aer_payload(
    hex_payload: &str,
    aer_base: u32,
    spikes: &mut [i8],
) -> Result<usize, SpikeTransportError> {
    let bytes = decode_hex_payload(hex_payload)?;
    apply_aer_payload(&bytes, aer_base, spikes)
}

/// Apply an AER payload to an existing spike vector.
pub fn apply_aer_payload(
    aer_payload: &[u8],
    aer_base: u32,
    spikes: &mut [i8],
) -> Result<usize, SpikeTransportError> {
    aer::decode_spikes(aer_payload, aer_base, spikes)
        .map_err(|_| SpikeTransportError::InvalidAerPayload)
}

/// Apply an iterator of spike indices to an existing spike vector.
pub fn apply_indices<I>(indices: I, spikes: &mut [i8]) -> usize
where
    I: IntoIterator,
    I::Item: TryInto<usize>,
{
    let mut count = 0usize;
    for raw in indices {
        let Ok(idx) = raw.try_into() else {
            continue;
        };
        if idx < spikes.len() {
            spikes[idx] = 1;
            count += 1;
        }
    }
    count
}

/// Apply `u32` spike indices to an existing spike vector.
pub fn apply_u32_indices(indices: &[u32], spikes: &mut [i8]) -> usize {
    apply_indices(indices.iter().copied(), spikes)
}

/// Apply `usize` spike indices to an existing spike vector.
pub fn apply_usize_indices(indices: &[usize], spikes: &mut [i8]) -> usize {
    apply_indices(indices.iter().copied(), spikes)
}

/// Build a spike vector from either AER bytes or direct indices.
///
/// If AER decoding succeeds it takes precedence. If it fails or is empty, direct
/// indices are applied as a fallback.
pub fn spikes_from_transport(
    aer_payload: &[u8],
    aer_base: u32,
    spike_indices: &[u32],
    len: usize,
) -> Result<Vec<i8>, SpikeTransportError> {
    let mut spikes = vec![0i8; len];
    let mut used_aer = false;
    if !aer_payload.is_empty() {
        match apply_aer_payload(aer_payload, aer_base, &mut spikes) {
            Ok(_) => used_aer = true,
            Err(err) if spike_indices.is_empty() => return Err(err),
            Err(_) => {}
        }
    }
    if !used_aer {
        apply_u32_indices(spike_indices, &mut spikes);
    }
    Ok(spikes)
}

/// Encode an exchange in both index and AER forms for downstream consumers.
pub fn encode_exchange(ts_us: u64, aer_base: u32, spikes: &[i8]) -> SpikeExchange {
    SpikeExchange {
        aer_payload: aer::encode_spikes(ts_us, aer_base, spikes),
        aer_base,
        spike_indices: spike_indices(spikes),
    }
}

/// Extract active spike indices from a spike vector.
pub fn spike_indices(spikes: &[i8]) -> Vec<u32> {
    spikes
        .iter()
        .enumerate()
        .filter_map(|(idx, &spike)| (spike != 0).then_some(idx as u32))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indices_are_applied_to_spike_vector() {
        let mut spikes = vec![0i8; 5];
        apply_u32_indices(&[1, 3, 99], &mut spikes);
        assert_eq!(spikes, vec![0, 1, 0, 1, 0]);
    }

    #[test]
    fn transport_falls_back_to_indices_when_aer_is_invalid() {
        let spikes = spikes_from_transport(b"bad", 0, &[2, 4], 5).unwrap();
        assert_eq!(spikes, vec![0, 0, 1, 0, 1]);
    }

    #[test]
    fn exchange_round_trips_indices_and_aer() {
        let encoded = encode_exchange(10, 4096, &[0, 1, 0, 1]);
        let decoded = spikes_from_transport(
            &encoded.aer_payload,
            encoded.aer_base,
            &encoded.spike_indices,
            4,
        )
        .unwrap();
        assert_eq!(decoded, vec![0, 1, 0, 1]);
        assert_eq!(encoded.spike_indices, vec![1, 3]);
    }
}
