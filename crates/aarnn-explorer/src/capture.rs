// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Complete activity capture and trace storage (brief Section 7).
//!
//! Modes:
//! - **off**: no recording, no allocation. This is the runner default and is
//!   not changed by the explorer.
//! - **scientific-isolated**: a trial runs on an isolated copy in a worker,
//!   and every event of the declared populations is written to checksummed
//!   segments with a manifest. Backpressure can only slow that experiment.
//! - **live-observation**: `LiveObserver` is a bounded, non-blocking queue.
//!   When it is full, the producer keeps running, and lost sequence ranges go
//!   to a gap list that cannot itself be dropped. A live stream with gaps is
//!   marked incomplete and never qualifies a deletion.
//!
//! Segment record (little endian, 24 bytes):
//! `id u64 | tick u64 | neuron u32 | micro u16 | kind u16 (0 spike, 1 graded)`.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{ExplorerError, Result, invalid};
use crate::exec::RunOutput;
use crate::gnat::{Event, EventKind};
use crate::store::{Project, sha256_hex};
use crate::tasks::{TaskDef, TrialResult};

pub const TRACE_SCHEMA: u32 = 1;
const RECORD: usize = 24;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Segment {
    pub sha256: String,
    pub first_seq: u64,
    pub last_seq: u64,
    pub count: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TraceManifest {
    pub schema_version: u32,
    pub source_sha256: String,
    pub topology_sha256: String,
    pub topology_generation: u64,
    pub task: String,
    pub seed: u64,
    pub plasticity_policy: String,
    pub dt_ms: f64,
    pub ticks: u64,
    pub mode: String,
    pub time_semantics: String,
    /// Population -> `complete` | `excluded: reason`.
    pub coverage: BTreeMap<String, String>,
    pub segments: Vec<Segment>,
    pub gaps: Vec<(u64, u64)>,
    pub excluded_events: u64,
    pub exclusion_reasons: Vec<String>,
    pub warmup_ticks: u64,
    pub complete: bool,
    pub output_series_sha256: String,
    pub output_digest: String,
    pub events_digest: String,
    pub trial: TrialResult,
}

/// Run one task trial with complete recording.
pub fn capture_trial(
    snapshot: &Value,
    task: &TaskDef,
    seed: u64,
    policy: &str,
    graded_events: bool,
) -> Result<(TrialResult, RunOutput)> {
    crate::tasks::run_trial(snapshot, task, seed, policy, None, true, graded_events)
}

pub fn encode(events: &[Event]) -> Vec<u8> {
    let mut out = Vec::with_capacity(events.len() * RECORD);
    for e in events {
        out.extend_from_slice(&e.id.to_le_bytes());
        out.extend_from_slice(&e.tick.to_le_bytes());
        out.extend_from_slice(&e.neuron.to_le_bytes());
        out.extend_from_slice(&(e.micro as u16).to_le_bytes());
        out.extend_from_slice(
            &(if e.kind == EventKind::GradedEquivalent {
                1u16
            } else {
                0
            })
            .to_le_bytes(),
        );
    }
    out
}

pub fn decode(bytes: &[u8]) -> Result<Vec<Event>> {
    if bytes.len() % RECORD != 0 {
        return invalid(format!(
            "segment length {} is not a multiple of {RECORD}",
            bytes.len()
        ));
    }
    let mut out = Vec::with_capacity(bytes.len() / RECORD);
    for r in bytes.chunks_exact(RECORD) {
        let u64_at = |o: usize| u64::from_le_bytes(r[o..o + 8].try_into().expect("8 bytes"));
        let kind = u16::from_le_bytes([r[22], r[23]]);
        out.push(Event {
            id: u64_at(0),
            tick: u64_at(8),
            neuron: u32::from_le_bytes(r[16..20].try_into().expect("4 bytes")),
            micro: u16::from_le_bytes([r[20], r[21]]) as u32,
            kind: match kind {
                0 => EventKind::Spike,
                1 => EventKind::GradedEquivalent,
                k => return invalid(format!("unknown event kind {k}")),
            },
        });
    }
    Ok(out)
}

/// Publish a recorded run as segments plus a manifest; returns the manifest
/// hash. Sequence numbers must be contiguous from 0.
#[allow(clippy::too_many_arguments)]
pub fn publish(
    project: &Project,
    source_sha256: &str,
    topology_sha256: &str,
    task: &str,
    trial: &TrialResult,
    run: &RunOutput,
    policy: &str,
    graded: bool,
    max_segment_mib: u64,
) -> Result<String> {
    for (k, e) in run.events.iter().enumerate() {
        if e.id != k as u64 {
            return Err(ExplorerError::Internal(format!(
                "trace sequence gap at {k} (id {})",
                e.id
            )));
        }
    }
    let per = ((max_segment_mib.max(1) << 20) as usize / RECORD).max(1);
    let mut segments = Vec::new();
    for chunk in run.events.chunks(per) {
        let bytes = encode(chunk);
        let h = project.put(&bytes)?;
        segments.push(Segment {
            sha256: h,
            first_seq: chunk[0].id,
            last_seq: chunk[chunk.len() - 1].id,
            count: chunk.len() as u64,
            bytes: bytes.len() as u64,
        });
    }
    let series = serde_json::to_vec(&run.output_series)?;
    let series_h = project.put(&series)?;
    let mut coverage = BTreeMap::new();
    coverage.insert("sensory".into(), "complete".into());
    coverage.insert("hidden-spiking".into(), "complete".into());
    coverage.insert(
        "hidden-graded".into(),
        if graded {
            "spike-equivalent events (effective-delivery-graded-v1)".into()
        } else {
            "excluded: graded cells emit no spikes; enable graded events".into()
        },
    );
    coverage.insert("outputs".into(), "complete".into());
    if run.excluded_events > 0 {
        coverage.insert(
            "newborn".into(),
            format!(
                "excluded: {} events outside topology generation 1",
                run.excluded_events
            ),
        );
    }
    let m = TraceManifest {
        schema_version: TRACE_SCHEMA,
        source_sha256: source_sha256.into(),
        topology_sha256: topology_sha256.into(),
        topology_generation: 1,
        task: task.into(),
        seed: trial.seed,
        plasticity_policy: policy.into(),
        dt_ms: 1.0,
        ticks: run.ticks,
        mode: "scientific-isolated".into(),
        time_semantics:
            "tick = runner step (1 ms); micro = within-step stage (0 sensory, 1 hidden, 2 output)"
                .into(),
        coverage,
        segments,
        gaps: Vec::new(),
        excluded_events: run.excluded_events,
        exclusion_reasons: run.exclusion_reasons.clone(),
        warmup_ticks: 0,
        complete: run.excluded_events == 0,
        output_series_sha256: series_h,
        output_digest: run.digest.clone(),
        events_digest: run.events_digest.clone(),
        trial: trial.clone(),
    };
    project.put_json(&m)
}

/// Load and verify a trace: checksums and contiguous sequence numbers.
pub fn load(project: &Project, manifest_hash: &str) -> Result<(TraceManifest, Vec<Event>)> {
    let m: TraceManifest = project.get_json(manifest_hash)?;
    if m.schema_version != TRACE_SCHEMA {
        return Err(ExplorerError::Unsupported(format!(
            "trace schema {}",
            m.schema_version
        )));
    }
    let mut events = Vec::new();
    let mut expect = 0u64;
    for s in &m.segments {
        let bytes = project.get(&s.sha256)?;
        let ev = decode(&bytes)?;
        if ev.len() as u64 != s.count
            || ev.first().map(|e| e.id) != Some(s.first_seq)
            || ev.last().map(|e| e.id) != Some(s.last_seq)
        {
            return invalid(format!(
                "segment {} does not match its manifest entry",
                s.sha256
            ));
        }
        for e in &ev {
            if e.id != expect {
                return invalid(format!(
                    "sequence discontinuity: expected {expect}, found {}",
                    e.id
                ));
            }
            expect += 1;
        }
        events.extend(ev);
    }
    Ok((m, events))
}

/// Bounded live observation queue. `offer` never blocks the producer. Lost
/// sequence ranges are recorded in a mutex-protected gap list, never in the
/// queue itself.
pub struct LiveObserver {
    tx: SyncSender<Vec<Event>>,
    pub dropped: Arc<AtomicU64>,
    pub gaps: Arc<Mutex<Vec<(u64, u64)>>>,
}

impl LiveObserver {
    pub fn new(capacity_batches: usize) -> (LiveObserver, Receiver<Vec<Event>>) {
        let (tx, rx) = sync_channel(capacity_batches.max(1));
        (
            LiveObserver {
                tx,
                dropped: Arc::new(AtomicU64::new(0)),
                gaps: Arc::new(Mutex::new(Vec::new())),
            },
            rx,
        )
    }

    /// Offer a batch; returns false when it was dropped (queue full or the
    /// consumer gone).
    pub fn offer(&self, batch: Vec<Event>) -> bool {
        let range = match (batch.first(), batch.last()) {
            (Some(a), Some(b)) => Some((a.id, b.id)),
            _ => None,
        };
        let n = batch.len() as u64;
        match self.tx.try_send(batch) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                self.dropped.fetch_add(n, Ordering::Relaxed);
                if let Some(r) = range {
                    if let Ok(mut g) = self.gaps.lock() {
                        g.push(r);
                    }
                }
                false
            }
        }
    }

    pub fn complete(&self) -> bool {
        self.dropped.load(Ordering::Relaxed) == 0
    }
}

/// Hash of a topology object (for manifests).
pub fn topology_hash(t: &crate::model::Topology) -> String {
    sha256_hex(serde_json::to_string(t).unwrap_or_default().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(id: u64, tick: u64, neuron: u32) -> Event {
        Event {
            id,
            tick,
            micro: 1,
            neuron,
            kind: EventKind::Spike,
        }
    }

    #[test]
    fn segment_roundtrip_and_corruption_detection() {
        let evs: Vec<Event> = (0..10).map(|i| ev(i, i * 3, (i % 4) as u32)).collect();
        let b = encode(&evs);
        assert_eq!(decode(&b).unwrap(), evs);
        assert!(decode(&b[..b.len() - 1]).is_err());
        let mut bad = b.clone();
        bad[22] = 9;
        assert!(decode(&bad).is_err(), "unknown event kind");
    }

    #[test]
    fn full_live_queue_records_gaps_without_blocking() {
        let (obs, rx) = LiveObserver::new(1);
        assert!(obs.offer(vec![ev(0, 0, 0), ev(1, 1, 0)]));
        assert!(
            !obs.offer(vec![ev(2, 2, 0), ev(3, 3, 0)]),
            "second batch must be dropped, not block"
        );
        assert_eq!(obs.dropped.load(Ordering::Relaxed), 2);
        assert_eq!(*obs.gaps.lock().unwrap(), vec![(2, 3)]);
        assert!(!obs.complete());
        drop(rx);
        assert!(!obs.offer(vec![ev(4, 4, 0)]));
    }
}
