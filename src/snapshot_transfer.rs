//! Bounded-frame transport for complete distributed network snapshots.
//!
//! The biological snapshot remains one logical, versioned document. This
//! adapter only changes its wire framing: every gRPC message stays small while
//! the total transfer can grow with the network and available storage.

use crate::distributed::proto::{
    NetworkSnapshotChunk, NetworkSnapshotResponse, NetworkSnapshotTransferMetadata,
};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use tokio_stream::StreamExt;
use tonic::Status;

pub const SNAPSHOT_TRANSFER_CHUNK_BYTES: usize = 1024 * 1024;
pub const SNAPSHOT_PAYLOAD_JSON: u32 = 1;
pub const SNAPSHOT_PAYLOAD_CHANNEL_STATE: u32 = 2;
pub const SNAPSHOT_PAYLOAD_AUTHORITATIVE_STATE: u32 = 3;

fn digest(payload: &[u8]) -> Vec<u8> {
    Sha256::digest(payload).to_vec()
}

/// Lazily splits a response into bounded protobuf frames without retaining a
/// second copy of the complete serialized snapshot.
pub struct SnapshotChunkEncoder {
    metadata: Option<NetworkSnapshotTransferMetadata>,
    payloads: VecDeque<(u32, String)>,
    current: Option<(u32, String, usize)>,
    sequence: u64,
    emitted_empty_frame: bool,
}

impl SnapshotChunkEncoder {
    pub fn new(response: NetworkSnapshotResponse) -> Self {
        let metadata = NetworkSnapshotTransferMetadata {
            network_id: response.network_id,
            step: response.step,
            sim_time_ms_bits: response.sim_time_ms_bits,
            cut_epoch: response.cut_epoch,
            participant_json: response.participant_json,
            channel_marker_json: response.channel_marker_json,
            snapshot_bytes: response.snapshot_json.len() as u64,
            snapshot_sha256: digest(response.snapshot_json.as_bytes()),
            channel_state_bytes: response.channel_state_json.len() as u64,
            channel_state_sha256: digest(response.channel_state_json.as_bytes()),
            authoritative_state_bytes: response.authoritative_state_json.len() as u64,
            authoritative_state_sha256: digest(response.authoritative_state_json.as_bytes()),
        };
        let payloads = [
            (SNAPSHOT_PAYLOAD_JSON, response.snapshot_json),
            (SNAPSHOT_PAYLOAD_CHANNEL_STATE, response.channel_state_json),
            (
                SNAPSHOT_PAYLOAD_AUTHORITATIVE_STATE,
                response.authoritative_state_json,
            ),
        ]
        .into_iter()
        .filter(|(_, payload)| !payload.is_empty())
        .collect();

        Self {
            metadata: Some(metadata),
            payloads,
            current: None,
            sequence: 0,
            emitted_empty_frame: false,
        }
    }
}

impl Iterator for SnapshotChunkEncoder {
    type Item = NetworkSnapshotChunk;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current.is_none() {
            self.current = self
                .payloads
                .pop_front()
                .map(|(kind, payload)| (kind, payload, 0));
        }

        let Some((kind, payload, offset)) = self.current.as_mut() else {
            // An empty sentinel is needed only when every payload is empty.
            // Non-empty transfers already mark their final payload frame and
            // must not receive a second, out-of-sequence terminal frame.
            if self.sequence > 0 {
                return None;
            }
            if self.emitted_empty_frame {
                return None;
            }
            self.emitted_empty_frame = true;
            return Some(NetworkSnapshotChunk {
                sequence: 0,
                payload_kind: 0,
                offset: 0,
                total_bytes: 0,
                payload: Vec::new(),
                metadata: self.metadata.take(),
                final_chunk: true,
            });
        };

        let end = offset
            .saturating_add(SNAPSHOT_TRANSFER_CHUNK_BYTES)
            .min(payload.len());
        let data = payload.as_bytes()[*offset..end].to_vec();
        let is_payload_end = end == payload.len();
        let total_bytes = payload.len() as u64;
        let chunk = NetworkSnapshotChunk {
            sequence: self.sequence,
            payload_kind: *kind,
            offset: *offset as u64,
            total_bytes,
            payload: data,
            metadata: self.metadata.take(),
            final_chunk: is_payload_end && self.payloads.is_empty(),
        };
        self.sequence = self.sequence.saturating_add(1);
        if is_payload_end {
            self.current = None;
        } else {
            *offset = end;
        }
        Some(chunk)
    }
}

#[derive(Default)]
pub struct SnapshotChunkAssembler {
    metadata: Option<NetworkSnapshotTransferMetadata>,
    snapshot: Vec<u8>,
    channel_state: Vec<u8>,
    authoritative_state: Vec<u8>,
    next_sequence: u64,
    finished: bool,
}

impl SnapshotChunkAssembler {
    pub fn push(&mut self, chunk: NetworkSnapshotChunk) -> Result<(), String> {
        if self.finished || chunk.sequence != self.next_sequence {
            return Err("snapshot chunk sequence is invalid".to_owned());
        }
        self.next_sequence = self.next_sequence.saturating_add(1);

        if self.next_sequence == 1 {
            let metadata = chunk
                .metadata
                .ok_or_else(|| "snapshot stream omitted transfer metadata".to_owned())?;
            if metadata.network_id.trim().is_empty() {
                return Err("snapshot transfer has an empty network ID".to_owned());
            }
            self.metadata = Some(metadata);
        } else if chunk.metadata.is_some() {
            return Err("snapshot metadata appeared after the first chunk".to_owned());
        }

        let Some(metadata) = self.metadata.as_ref() else {
            return Err("snapshot transfer metadata is unavailable".to_owned());
        };
        let expected_total = match chunk.payload_kind {
            0 if self.next_sequence == 1 && chunk.final_chunk => {
                self.finished = true;
                return Ok(());
            }
            SNAPSHOT_PAYLOAD_JSON => metadata.snapshot_bytes,
            SNAPSHOT_PAYLOAD_CHANNEL_STATE => metadata.channel_state_bytes,
            SNAPSHOT_PAYLOAD_AUTHORITATIVE_STATE => metadata.authoritative_state_bytes,
            _ => return Err("snapshot chunk payload kind is invalid".to_owned()),
        };
        let target = match chunk.payload_kind {
            SNAPSHOT_PAYLOAD_JSON => &mut self.snapshot,
            SNAPSHOT_PAYLOAD_CHANNEL_STATE => &mut self.channel_state,
            SNAPSHOT_PAYLOAD_AUTHORITATIVE_STATE => &mut self.authoritative_state,
            _ => return Err("snapshot chunk payload kind is invalid".to_owned()),
        };
        if chunk.total_bytes != expected_total
            || chunk.offset != target.len() as u64
            || chunk.payload.is_empty()
            || chunk.payload.len() > SNAPSHOT_TRANSFER_CHUNK_BYTES
        {
            return Err("snapshot chunk bounds are invalid".to_owned());
        }
        let end = target
            .len()
            .checked_add(chunk.payload.len())
            .ok_or_else(|| "snapshot payload size overflow".to_owned())?;
        if end as u64 > expected_total {
            return Err("snapshot chunk exceeds declared payload length".to_owned());
        }
        target
            .try_reserve(chunk.payload.len())
            .map_err(|error| format!("snapshot payload allocation failed: {error}"))?;
        target.extend_from_slice(&chunk.payload);
        if chunk.final_chunk {
            self.finished = true;
        }
        Ok(())
    }

    pub fn finish(self) -> Result<NetworkSnapshotResponse, String> {
        let metadata = self
            .metadata
            .ok_or_else(|| "snapshot stream is empty".to_owned())?;
        if !self.finished
            || self.snapshot.len() as u64 != metadata.snapshot_bytes
            || self.channel_state.len() as u64 != metadata.channel_state_bytes
            || self.authoritative_state.len() as u64 != metadata.authoritative_state_bytes
            || digest(&self.snapshot) != metadata.snapshot_sha256
            || digest(&self.channel_state) != metadata.channel_state_sha256
            || digest(&self.authoritative_state) != metadata.authoritative_state_sha256
        {
            return Err("snapshot stream is incomplete or failed digest verification".to_owned());
        }
        Ok(NetworkSnapshotResponse {
            network_id: metadata.network_id,
            snapshot_json: String::from_utf8(self.snapshot)
                .map_err(|error| format!("snapshot JSON is not UTF-8: {error}"))?,
            step: metadata.step,
            sim_time_ms_bits: metadata.sim_time_ms_bits,
            channel_state_json: String::from_utf8(self.channel_state)
                .map_err(|error| format!("channel state is not UTF-8: {error}"))?,
            cut_epoch: metadata.cut_epoch,
            participant_json: metadata.participant_json,
            channel_marker_json: metadata.channel_marker_json,
            authoritative_state_json: String::from_utf8(self.authoritative_state)
                .map_err(|error| format!("authoritative state is not UTF-8: {error}"))?,
        })
    }
}

pub async fn collect_snapshot_stream(
    mut stream: tonic::Streaming<NetworkSnapshotChunk>,
) -> Result<NetworkSnapshotResponse, String> {
    let mut assembler = SnapshotChunkAssembler::default();
    while let Some(chunk) = stream
        .next()
        .await
        .transpose()
        .map_err(|error: Status| error.to_string())?
    {
        assembler.push(chunk)?;
    }
    assembler.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(snapshot_json: String) -> NetworkSnapshotResponse {
        NetworkSnapshotResponse {
            network_id: "shared-network".to_owned(),
            snapshot_json,
            step: 42,
            sim_time_ms_bits: 1.5_f64.to_bits(),
            channel_state_json: "{\"channel\":true}".to_owned(),
            cut_epoch: 7,
            participant_json: "{\"participant\":true}".to_owned(),
            channel_marker_json: "{\"marker\":true}".to_owned(),
            authoritative_state_json: "{\"authoritative\":true}".to_owned(),
        }
    }

    #[test]
    fn snapshot_transfer_round_trips_multiple_payloads_and_frames() {
        let expected = response("x".repeat(SNAPSHOT_TRANSFER_CHUNK_BYTES + 17));
        let chunks = SnapshotChunkEncoder::new(expected.clone()).collect::<Vec<_>>();
        assert_eq!(chunks.len(), 4);
        assert!(
            chunks
                .iter()
                .all(|chunk| chunk.payload.len() <= SNAPSHOT_TRANSFER_CHUNK_BYTES)
        );

        let mut assembler = SnapshotChunkAssembler::default();
        for chunk in chunks {
            assembler.push(chunk).expect("valid snapshot chunk");
        }
        assert_eq!(assembler.finish().expect("complete snapshot"), expected);
    }

    #[test]
    fn empty_snapshot_transfer_uses_one_metadata_sentinel_frame() {
        let expected = NetworkSnapshotResponse {
            network_id: "empty-network".to_owned(),
            snapshot_json: String::new(),
            step: 0,
            sim_time_ms_bits: 0.0_f64.to_bits(),
            channel_state_json: String::new(),
            cut_epoch: 0,
            participant_json: String::new(),
            channel_marker_json: String::new(),
            authoritative_state_json: String::new(),
        };
        let chunks = SnapshotChunkEncoder::new(expected.clone()).collect::<Vec<_>>();
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].metadata.is_some());
        assert!(chunks[0].final_chunk);

        let mut assembler = SnapshotChunkAssembler::default();
        assembler
            .push(chunks.into_iter().next().expect("sentinel frame"))
            .expect("valid empty transfer");
        assert_eq!(assembler.finish().expect("complete snapshot"), expected);
    }

    #[test]
    fn snapshot_transfer_chunks_a_payload_larger_than_sixty_four_mib() {
        let bytes = 64 * 1024 * 1024 + 1;
        let expected = response("x".repeat(bytes));
        let total_bytes = expected.snapshot_json.len()
            + expected.channel_state_json.len()
            + expected.authoritative_state_json.len();
        let encoder = SnapshotChunkEncoder::new(expected);
        let mut count = 0;
        let mut total = 0usize;
        for chunk in encoder {
            assert!(chunk.payload.len() <= SNAPSHOT_TRANSFER_CHUNK_BYTES);
            total += chunk.payload.len();
            count += 1;
        }
        assert_eq!(total, total_bytes);
        assert_eq!(count, bytes.div_ceil(SNAPSHOT_TRANSFER_CHUNK_BYTES) + 2);
    }

    #[test]
    fn snapshot_transfer_rejects_a_modified_payload() {
        let mut chunks = SnapshotChunkEncoder::new(response("{}".to_owned()));
        let mut chunk = chunks.next().expect("snapshot chunk");
        chunk.payload[0] ^= 1;
        let mut assembler = SnapshotChunkAssembler::default();
        assembler.push(chunk).expect("framing remains valid");
        for chunk in chunks {
            assembler.push(chunk).expect("framing remains valid");
        }
        assert!(assembler.finish().is_err());
    }
}
