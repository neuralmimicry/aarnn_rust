//! Asynchronous global-virtual-time and consistent-cut coordination.
//!
//! A cut is complete only after every participant has reported and every
//! channel has recorded the cut marker.  The minimum includes local work,
//! queued work and in-flight channel work.  This is a bounded reference
//! implementation of a Chandy--Lamport-style control protocol; it does not
//! stop biological execution or infer termination from silence.

use crate::deterministic::{LogicalTag, StateDigest, StateDigestBuilder};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const CONSISTENT_CUT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsistentCutMessage {
    Participant(ParticipantReport),
    Marker(ChannelMarker),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParticipantReport {
    pub participant: String,
    pub local_frontier: LogicalTag,
    pub queued_min: Option<LogicalTag>,
    pub in_flight_min: Option<LogicalTag>,
    pub activity_epoch: u64,
}

impl ParticipantReport {
    pub fn validate(&self) -> Result<(), ConsistentCutError> {
        if self.participant.trim().is_empty() {
            return Err(ConsistentCutError::InvalidParticipant);
        }
        if self.activity_epoch == 0 {
            return Err(ConsistentCutError::InvalidActivityEpoch {
                participant: self.participant.clone(),
            });
        }
        Ok(())
    }

    fn minimum(&self) -> LogicalTag {
        [
            Some(self.local_frontier),
            self.queued_min,
            self.in_flight_min,
        ]
        .into_iter()
        .flatten()
        .min()
        .expect("local frontier is always present")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelMarker {
    pub channel: String,
    pub epoch: u64,
    pub first_in_transit: Option<LogicalTag>,
    pub captured_digest: StateDigest,
}

impl ChannelMarker {
    pub fn new(
        channel: impl Into<String>,
        epoch: u64,
        first_in_transit: Option<LogicalTag>,
        channel_state: &[u8],
    ) -> Result<Self, ConsistentCutError> {
        let channel = channel.into();
        if channel.trim().is_empty() || epoch == 0 {
            return Err(ConsistentCutError::InvalidMarker);
        }
        let mut digest = StateDigestBuilder::default();
        digest.add_domain("consistent-cut-channel:v1", channel_state);
        Ok(Self {
            channel,
            epoch,
            first_in_transit,
            captured_digest: digest.finish(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsistentCut {
    pub schema_version: u32,
    pub epoch: u64,
    pub safe_tag: LogicalTag,
    pub participants: Vec<ParticipantReport>,
    pub channels: Vec<ChannelMarker>,
    pub cut_digest: StateDigest,
}

impl ConsistentCut {
    pub fn verify(&self) -> Result<(), ConsistentCutError> {
        if self.schema_version != CONSISTENT_CUT_SCHEMA_VERSION || self.epoch == 0 {
            return Err(ConsistentCutError::InvalidCut);
        }
        let mut participants = BTreeSet::new();
        for report in &self.participants {
            report.validate()?;
            if !participants.insert(report.participant.as_str()) {
                return Err(ConsistentCutError::DuplicateParticipant(
                    report.participant.clone(),
                ));
            }
            if report.minimum() < self.safe_tag {
                return Err(ConsistentCutError::InvalidCut);
            }
        }
        let mut channels = BTreeSet::new();
        for marker in &self.channels {
            if marker.epoch != self.epoch || !channels.insert(marker.channel.as_str()) {
                return Err(ConsistentCutError::InvalidMarker);
            }
        }
        let mut digest = StateDigestBuilder::default();
        digest.add_domain("consistent-cut-epoch", self.epoch.to_be_bytes());
        digest.add_domain(
            "consistent-cut-safe-tag",
            self.safe_tag.to_string().as_bytes(),
        );
        for report in &self.participants {
            digest.add_domain(
                format!("participant:{}", report.participant),
                serde_json::to_vec(report)
                    .map_err(|_| ConsistentCutError::Encoding)?
                    .as_slice(),
            );
        }
        for marker in &self.channels {
            digest.add_domain(
                format!("channel:{}", marker.channel),
                serde_json::to_vec(marker)
                    .map_err(|_| ConsistentCutError::Encoding)?
                    .as_slice(),
            );
        }
        if digest.finish() != self.cut_digest {
            return Err(ConsistentCutError::DigestMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConsistentCutError {
    #[error("participant set is empty")]
    EmptyParticipants,
    #[error("participant identity is invalid")]
    InvalidParticipant,
    #[error("participant {participant} reported an invalid activity epoch")]
    InvalidActivityEpoch { participant: String },
    #[error("participant {0} reported more than once in a cut epoch")]
    DuplicateParticipant(String),
    #[error("cut marker is invalid or belongs to another epoch")]
    InvalidMarker,
    #[error("cut epoch is not complete")]
    Incomplete,
    #[error("consistent cut is invalid")]
    InvalidCut,
    #[error("consistent cut digest does not match its contents")]
    DigestMismatch,
    #[error("consistent cut encoding failed")]
    Encoding,
    #[error("consistent-cut persistence failed: {0}")]
    Persistence(String),
    #[error("consistent cut with digest {0} is already published")]
    AlreadyPublished(StateDigest),
    #[error("consistent cut with digest {0} is not available")]
    MissingCut(StateDigest),
}

/// Crash-safe evidence store for asynchronous cut epochs.
///
/// Coordinator state is replaced as reports/markers arrive so a control-plane
/// restart can resume a partial epoch. Final cuts are content-addressed and
/// published without replacement; an existing digest is therefore immutable
/// evidence rather than a mutable status record.
#[derive(Debug, Clone)]
pub struct FileConsistentCutStore {
    root: PathBuf,
}

impl FileConsistentCutStore {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, ConsistentCutError> {
        let root = root.into();
        fs::create_dir_all(&root)
            .map_err(|error| ConsistentCutError::Persistence(error.to_string()))?;
        Ok(Self { root })
    }

    /// Allocate a monotonically increasing control-plane epoch. This number
    /// is deliberately separate from biological/logical time.
    pub fn next_epoch(&self) -> Result<u64, ConsistentCutError> {
        let lock_path = self.root.join("epoch.lock");
        let lock = OpenOptions::new()
            .write(true)
            .create(true)
            .open(&lock_path)
            .map_err(|error| ConsistentCutError::Persistence(error.to_string()))?;
        fs2::FileExt::lock_exclusive(&lock)
            .map_err(|error| ConsistentCutError::Persistence(error.to_string()))?;
        let result = (|| {
            let path = self.root.join("epoch.json");
            let current = match fs::read(&path) {
                Ok(bytes) => serde_json::from_slice::<u64>(&bytes)
                    .map_err(|error| ConsistentCutError::Persistence(error.to_string()))?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
                Err(error) => return Err(ConsistentCutError::Persistence(error.to_string())),
            };
            let next = current
                .checked_add(1)
                .ok_or(ConsistentCutError::InvalidCut)?;
            atomic_replace(
                &path,
                &serde_json::to_vec(&next)
                    .map_err(|error| ConsistentCutError::Persistence(error.to_string()))?,
            )?;
            Ok(next)
        })();
        let unlock = fs2::FileExt::unlock(&lock)
            .map_err(|error| ConsistentCutError::Persistence(error.to_string()));
        match (result, unlock) {
            (Ok(epoch), Ok(())) => Ok(epoch),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
        }
    }

    pub fn begin(
        &self,
        epoch: u64,
        participants: impl IntoIterator<Item = String>,
        channels: impl IntoIterator<Item = String>,
    ) -> Result<PersistedConsistentCutCoordinator, ConsistentCutError> {
        let coordinator = ConsistentCutCoordinator::begin(epoch, participants, channels)?;
        let persisted = Self::coordinator_path_for(&self.root, epoch);
        if persisted.exists() {
            return Err(ConsistentCutError::Persistence(format!(
                "coordinator epoch {epoch} already exists; resume it instead"
            )));
        }
        self.persist_coordinator(&coordinator)?;
        Ok(PersistedConsistentCutCoordinator {
            store: self.clone(),
            coordinator,
        })
    }

    pub fn resume(
        &self,
        epoch: u64,
    ) -> Result<PersistedConsistentCutCoordinator, ConsistentCutError> {
        let path = Self::coordinator_path_for(&self.root, epoch);
        let bytes = fs::read(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ConsistentCutError::Persistence(format!(
                    "coordinator epoch {epoch} is not available"
                ))
            } else {
                ConsistentCutError::Persistence(error.to_string())
            }
        })?;
        let coordinator: ConsistentCutCoordinator = serde_json::from_slice(&bytes)
            .map_err(|error| ConsistentCutError::Persistence(error.to_string()))?;
        coordinator.validate_state()?;
        if coordinator.epoch != epoch {
            return Err(ConsistentCutError::Persistence(
                "coordinator epoch does not match its publication path".to_owned(),
            ));
        }
        Ok(PersistedConsistentCutCoordinator {
            store: self.clone(),
            coordinator,
        })
    }

    pub fn publish(&self, cut: &ConsistentCut) -> Result<PathBuf, ConsistentCutError> {
        cut.verify()?;
        let path = self
            .root
            .join(format!("consistent-cut-{}.json", cut.cut_digest));
        let bytes = serde_json::to_vec(cut).map_err(|_| ConsistentCutError::Encoding)?;
        atomic_publish_no_replace(&path, &bytes, cut.cut_digest)?;
        Ok(path)
    }

    pub fn load(&self, digest: StateDigest) -> Result<ConsistentCut, ConsistentCutError> {
        let path = self.root.join(format!("consistent-cut-{digest}.json"));
        let bytes = fs::read(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ConsistentCutError::MissingCut(digest)
            } else {
                ConsistentCutError::Persistence(error.to_string())
            }
        })?;
        let cut: ConsistentCut = serde_json::from_slice(&bytes)
            .map_err(|error| ConsistentCutError::Persistence(error.to_string()))?;
        if cut.cut_digest != digest {
            return Err(ConsistentCutError::DigestMismatch);
        }
        cut.verify()?;
        Ok(cut)
    }

    fn coordinator_path_for(root: &Path, epoch: u64) -> PathBuf {
        root.join(format!("consistent-cut-epoch-{epoch}.json"))
    }

    fn persist_coordinator(
        &self,
        coordinator: &ConsistentCutCoordinator,
    ) -> Result<(), ConsistentCutError> {
        let path = Self::coordinator_path_for(&self.root, coordinator.epoch);
        let bytes = serde_json::to_vec(coordinator).map_err(|_| ConsistentCutError::Encoding)?;
        atomic_replace(&path, &bytes)
    }
}

/// A cut coordinator whose partial report/marker state is persisted after
/// each accepted message. It is intentionally usable by independent async
/// tasks through ownership transfer and never completes from channel silence.
#[derive(Debug)]
pub struct PersistedConsistentCutCoordinator {
    store: FileConsistentCutStore,
    coordinator: ConsistentCutCoordinator,
}

impl PersistedConsistentCutCoordinator {
    pub fn epoch(&self) -> u64 {
        self.coordinator.epoch
    }

    pub fn is_complete(&self) -> bool {
        self.coordinator.is_complete()
    }

    pub fn record_message(
        &mut self,
        message: ConsistentCutMessage,
    ) -> Result<(), ConsistentCutError> {
        let previous = self.coordinator.clone();
        self.coordinator.record_message(message)?;
        if let Err(error) = self.store.persist_coordinator(&self.coordinator) {
            self.coordinator = previous;
            return Err(error);
        }
        Ok(())
    }

    pub fn finalise(&self) -> Result<ConsistentCut, ConsistentCutError> {
        self.coordinator.finalise()
    }

    pub fn finalise_and_publish(&self) -> Result<(ConsistentCut, PathBuf), ConsistentCutError> {
        let cut = self.finalise()?;
        let path = self.store.publish(&cut)?;
        Ok((cut, path))
    }
}

fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<(), ConsistentCutError> {
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temporary)
        .map_err(|error| ConsistentCutError::Persistence(error.to_string()))?;
    if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(&temporary);
        return Err(ConsistentCutError::Persistence(error.to_string()));
    }
    fs::rename(&temporary, path)
        .map_err(|error| ConsistentCutError::Persistence(error.to_string()))?;
    if let Some(parent) = path.parent() {
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| ConsistentCutError::Persistence(error.to_string()))?;
    }
    Ok(())
}

fn atomic_publish_no_replace(
    path: &Path,
    bytes: &[u8],
    digest: StateDigest,
) -> Result<(), ConsistentCutError> {
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| ConsistentCutError::Persistence(error.to_string()))?;
    if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(&temporary);
        return Err(ConsistentCutError::Persistence(error.to_string()));
    }
    let result = fs::hard_link(&temporary, path);
    let _ = fs::remove_file(&temporary);
    match result {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(ConsistentCutError::AlreadyPublished(digest));
        }
        Err(error) => return Err(ConsistentCutError::Persistence(error.to_string())),
    }
    if let Some(parent) = path.parent() {
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| ConsistentCutError::Persistence(error.to_string()))?;
    }
    Ok(())
}

/// Coordinator state is intentionally serialisable so a control-plane
/// restart can reject partial epochs rather than publishing a false cut.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsistentCutCoordinator {
    epoch: u64,
    expected_participants: BTreeSet<String>,
    expected_channels: BTreeSet<String>,
    reports: BTreeMap<String, ParticipantReport>,
    markers: BTreeMap<String, ChannelMarker>,
}

impl ConsistentCutCoordinator {
    pub fn begin(
        epoch: u64,
        participants: impl IntoIterator<Item = String>,
        channels: impl IntoIterator<Item = String>,
    ) -> Result<Self, ConsistentCutError> {
        let expected_participants = participants.into_iter().collect::<BTreeSet<_>>();
        if epoch == 0 || expected_participants.is_empty() {
            return Err(ConsistentCutError::EmptyParticipants);
        }
        let expected_channels = channels.into_iter().collect::<BTreeSet<_>>();
        if expected_participants.iter().any(|id| id.trim().is_empty())
            || expected_channels.iter().any(|id| id.trim().is_empty())
        {
            return Err(ConsistentCutError::InvalidParticipant);
        }
        Ok(Self {
            epoch,
            expected_participants,
            expected_channels,
            reports: BTreeMap::new(),
            markers: BTreeMap::new(),
        })
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    fn validate_state(&self) -> Result<(), ConsistentCutError> {
        if self.epoch == 0 || self.expected_participants.is_empty() {
            return Err(ConsistentCutError::InvalidCut);
        }
        if self
            .expected_participants
            .iter()
            .any(|id| id.trim().is_empty())
            || self.expected_channels.iter().any(|id| id.trim().is_empty())
            || self
                .reports
                .keys()
                .any(|id| !self.expected_participants.contains(id))
            || self
                .markers
                .keys()
                .any(|id| !self.expected_channels.contains(id))
        {
            return Err(ConsistentCutError::InvalidCut);
        }
        for report in self.reports.values() {
            report.validate()?;
        }
        for marker in self.markers.values() {
            if marker.epoch != self.epoch || marker.channel.trim().is_empty() {
                return Err(ConsistentCutError::InvalidMarker);
            }
        }
        Ok(())
    }

    pub fn record_report(&mut self, report: ParticipantReport) -> Result<(), ConsistentCutError> {
        report.validate()?;
        if !self.expected_participants.contains(&report.participant) {
            return Err(ConsistentCutError::InvalidParticipant);
        }
        if self
            .reports
            .insert(report.participant.clone(), report)
            .is_some()
        {
            return Err(ConsistentCutError::DuplicateParticipant(
                "duplicate report".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn record_marker(&mut self, marker: ChannelMarker) -> Result<(), ConsistentCutError> {
        if marker.epoch != self.epoch || !self.expected_channels.contains(&marker.channel) {
            return Err(ConsistentCutError::InvalidMarker);
        }
        if self
            .markers
            .insert(marker.channel.clone(), marker)
            .is_some()
        {
            return Err(ConsistentCutError::InvalidMarker);
        }
        Ok(())
    }

    pub fn record_message(
        &mut self,
        message: ConsistentCutMessage,
    ) -> Result<(), ConsistentCutError> {
        match message {
            ConsistentCutMessage::Participant(report) => self.record_report(report),
            ConsistentCutMessage::Marker(marker) => self.record_marker(marker),
        }
    }

    pub fn is_complete(&self) -> bool {
        self.reports.len() == self.expected_participants.len()
            && self.markers.len() == self.expected_channels.len()
    }

    /// Finalise only after all marker records arrive. The safe tag is the
    /// minimum of participant frontiers and all queued/in-flight evidence;
    /// it is not a termination decision and does not wait for equal ticks.
    pub fn finalise(&self) -> Result<ConsistentCut, ConsistentCutError> {
        if !self.is_complete() {
            return Err(ConsistentCutError::Incomplete);
        }
        let participants = self.reports.values().cloned().collect::<Vec<_>>();
        let channels = self.markers.values().cloned().collect::<Vec<_>>();
        let safe_tag = participants
            .iter()
            .map(ParticipantReport::minimum)
            .chain(channels.iter().filter_map(|marker| marker.first_in_transit))
            .min()
            .ok_or(ConsistentCutError::EmptyParticipants)?;
        let mut digest = StateDigestBuilder::default();
        digest.add_domain("consistent-cut-epoch", self.epoch.to_be_bytes());
        digest.add_domain("consistent-cut-safe-tag", safe_tag.to_string().as_bytes());
        for report in &participants {
            digest.add_domain(
                format!("participant:{}", report.participant),
                serde_json::to_vec(report)
                    .map_err(|_| ConsistentCutError::Encoding)?
                    .as_slice(),
            );
        }
        for marker in &channels {
            digest.add_domain(
                format!("channel:{}", marker.channel),
                serde_json::to_vec(marker)
                    .map_err(|_| ConsistentCutError::Encoding)?
                    .as_slice(),
            );
        }
        let cut = ConsistentCut {
            schema_version: CONSISTENT_CUT_SCHEMA_VERSION,
            epoch: self.epoch,
            safe_tag,
            participants,
            channels,
            cut_digest: digest.finish(),
        };
        cut.verify()?;
        Ok(cut)
    }

    /// Async facade used by orchestrator control tasks. It performs no
    /// blocking I/O and therefore does not turn GVT into a periodic execution
    /// barrier; reports and markers may be delivered by independent tasks.
    pub async fn finalise_async(&self) -> Result<ConsistentCut, ConsistentCutError> {
        self.finalise()
    }
}

/// Asynchronously receives cut reports and markers from independent
/// participants. The collector has no timer-based completion path: a closed
/// channel before all required evidence is received is an incomplete cut,
/// never quiescence or termination.
pub struct AsyncConsistentCutCollector {
    coordinator: AsyncCutCoordinator,
    messages: tokio::sync::mpsc::Receiver<ConsistentCutMessage>,
}

enum AsyncCutCoordinator {
    InMemory(ConsistentCutCoordinator),
    Persisted(PersistedConsistentCutCoordinator),
}

impl AsyncConsistentCutCollector {
    pub fn new(
        coordinator: ConsistentCutCoordinator,
        messages: tokio::sync::mpsc::Receiver<ConsistentCutMessage>,
    ) -> Self {
        Self {
            coordinator: AsyncCutCoordinator::InMemory(coordinator),
            messages,
        }
    }

    pub fn new_persisted(
        coordinator: PersistedConsistentCutCoordinator,
        messages: tokio::sync::mpsc::Receiver<ConsistentCutMessage>,
    ) -> Self {
        Self {
            coordinator: AsyncCutCoordinator::Persisted(coordinator),
            messages,
        }
    }

    fn is_complete(&self) -> bool {
        match &self.coordinator {
            AsyncCutCoordinator::InMemory(coordinator) => coordinator.is_complete(),
            AsyncCutCoordinator::Persisted(coordinator) => coordinator.is_complete(),
        }
    }

    pub async fn finalise(mut self) -> Result<ConsistentCut, ConsistentCutError> {
        while !self.is_complete() {
            let Some(message) = self.messages.recv().await else {
                return Err(ConsistentCutError::Incomplete);
            };
            match &mut self.coordinator {
                AsyncCutCoordinator::InMemory(coordinator) => {
                    coordinator.record_message(message)?
                }
                AsyncCutCoordinator::Persisted(coordinator) => {
                    coordinator.record_message(message)?
                }
            }
        }
        match &self.coordinator {
            AsyncCutCoordinator::InMemory(coordinator) => coordinator.finalise(),
            AsyncCutCoordinator::Persisted(coordinator) => coordinator.finalise(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(
        id: &str,
        local: u64,
        queued: Option<u64>,
        inflight: Option<u64>,
    ) -> ParticipantReport {
        ParticipantReport {
            participant: id.to_owned(),
            local_frontier: LogicalTag::new(local, 0),
            queued_min: queued.map(|tick| LogicalTag::new(tick, 0)),
            in_flight_min: inflight.map(|tick| LogicalTag::new(tick, 0)),
            activity_epoch: 1,
        }
    }

    #[test]
    fn delayed_channel_blocks_gvt_and_queue_empty_is_not_termination() {
        let mut coordinator = ConsistentCutCoordinator::begin(
            1,
            ["a".to_owned(), "b".to_owned()],
            ["a-b".to_owned()],
        )
        .unwrap();
        coordinator
            .record_report(report("a", 10, None, None))
            .unwrap();
        coordinator
            .record_report(report("b", 12, None, None))
            .unwrap();
        assert!(!coordinator.is_complete());
        let marker =
            ChannelMarker::new("a-b", 1, Some(LogicalTag::new(4, 2)), b"delayed-message").unwrap();
        coordinator.record_marker(marker).unwrap();
        let cut = coordinator.finalise().unwrap();
        assert_eq!(cut.safe_tag, LogicalTag::new(4, 2));
    }

    #[test]
    fn report_reordering_does_not_change_cut_digest() {
        let build = |reverse: bool| {
            let mut c = ConsistentCutCoordinator::begin(
                7,
                ["a".to_owned(), "b".to_owned()],
                ["a-b".to_owned(), "b-a".to_owned()],
            )
            .unwrap();
            let reports = [
                report("a", 8, Some(9), None),
                report("b", 11, None, Some(10)),
            ];
            for item in if reverse {
                reports.into_iter().rev().collect::<Vec<_>>()
            } else {
                reports.into_iter().collect()
            } {
                c.record_report(item).unwrap();
            }
            for channel in ["a-b", "b-a"] {
                c.record_marker(ChannelMarker::new(channel, 7, None, channel.as_bytes()).unwrap())
                    .unwrap();
            }
            c.finalise().unwrap()
        };
        assert_eq!(build(false), build(true));
    }

    #[tokio::test]
    async fn async_collector_waits_for_all_independent_evidence() {
        let coordinator = ConsistentCutCoordinator::begin(
            9,
            ["a".to_owned(), "b".to_owned()],
            ["a-b".to_owned(), "b-a".to_owned()],
        )
        .unwrap();
        let (sender, receiver) = tokio::sync::mpsc::channel(4);
        sender
            .send(ConsistentCutMessage::Participant(report(
                "b", 8, None, None,
            )))
            .await
            .unwrap();
        sender
            .send(ConsistentCutMessage::Marker(
                ChannelMarker::new("b-a", 9, None, b"b-a").unwrap(),
            ))
            .await
            .unwrap();
        sender
            .send(ConsistentCutMessage::Participant(report(
                "a", 7, None, None,
            )))
            .await
            .unwrap();
        sender
            .send(ConsistentCutMessage::Marker(
                ChannelMarker::new("a-b", 9, Some(LogicalTag::new(5, 1)), b"a-b").unwrap(),
            ))
            .await
            .unwrap();
        drop(sender);
        let cut = AsyncConsistentCutCollector::new(coordinator, receiver)
            .finalise()
            .await
            .unwrap();
        assert_eq!(cut.safe_tag, LogicalTag::new(5, 1));
    }

    #[test]
    fn persisted_cut_epoch_resumes_and_publishes_immutable_evidence() {
        let root =
            std::env::temp_dir().join(format!("aarnn-consistent-cut-store-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let store = FileConsistentCutStore::new(&root).unwrap();
        assert_eq!(store.next_epoch().unwrap(), 1);
        assert_eq!(store.next_epoch().unwrap(), 2);

        let mut coordinator = store
            .begin(
                2,
                ["a".to_owned(), "b".to_owned()],
                ["a-b".to_owned(), "b-a".to_owned()],
            )
            .unwrap();
        coordinator
            .record_message(ConsistentCutMessage::Participant(report(
                "a", 10, None, None,
            )))
            .unwrap();
        drop(coordinator);

        let mut resumed = store.resume(2).unwrap();
        assert!(!resumed.is_complete());
        resumed
            .record_message(ConsistentCutMessage::Participant(report(
                "b", 11, None, None,
            )))
            .unwrap();
        for channel in ["a-b", "b-a"] {
            resumed
                .record_message(ConsistentCutMessage::Marker(
                    ChannelMarker::new(channel, 2, None, channel.as_bytes()).unwrap(),
                ))
                .unwrap();
        }
        let (cut, path) = resumed.finalise_and_publish().unwrap();
        assert!(path.is_file());
        assert_eq!(store.load(cut.cut_digest).unwrap(), cut);
        assert!(matches!(
            store.publish(&cut),
            Err(ConsistentCutError::AlreadyPublished(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }
}
