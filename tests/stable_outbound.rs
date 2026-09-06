use aarnn_rust::causal::CausalEvent;
use aarnn_rust::deterministic::{
    BrainId, CanonicalEventKey, EventId, EventStage, LeaseTerm, LogicalTag, NeuronId, ShardId,
    StateDigest,
};
use aarnn_rust::partial_shard_executor::PartialShardOutbound;
use aarnn_rust::shard_executor::RoutedCausalEvent;
use aarnn_rust::stable_outbound::{
    StableOutboundAcknowledgement, StableOutboundError, StableOutboundLog,
};

fn temporary_path(label: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "aarnn-stable-outbound-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&path).unwrap();
    path.join("outbound.json")
}

fn message(event_id: u64, destination_shard: u64) -> PartialShardOutbound {
    let event = EventId::new(event_id).unwrap();
    let target = NeuronId::new(destination_shard).unwrap();
    PartialShardOutbound::CausalEvent {
        plan_digest: StateDigest([7; 16]),
        destination_shard: ShardId::new(destination_shard).unwrap(),
        event: RoutedCausalEvent {
            route: None,
            event: CausalEvent {
                key: CanonicalEventKey::new(
                    LogicalTag::ZERO,
                    EventStage::SynapticTransition,
                    0,
                    target.raw(),
                    event.raw(),
                ),
                id: event,
                payload: vec![1, 2, 3],
                original_tag: LogicalTag::ZERO,
                deferred_from_nonconvergence: false,
            },
        },
    }
}

#[test]
fn append_reopen_retry_and_ack_preserve_per_destination_sequences() {
    let path = temporary_path("replay");
    let brain = BrainId::new(77).unwrap();
    let mut log = StableOutboundLog::open(&path, brain, 8).unwrap();
    let first = log
        .append("worker-a", LeaseTerm::INITIAL, 1, message(1, 1))
        .unwrap();
    let second = log
        .append("worker-a", LeaseTerm::INITIAL, 1, message(2, 1))
        .unwrap();
    let other = log
        .append("worker-b", LeaseTerm::INITIAL, 1, message(3, 2))
        .unwrap();
    assert_eq!(first.sequence, 0);
    assert_eq!(second.sequence, 1);
    assert_eq!(other.sequence, 0);

    let mut reopened = StableOutboundLog::open(&path, brain, 8).unwrap();
    assert_eq!(reopened.pending("worker-a").unwrap().len(), 2);
    assert_eq!(reopened.pending("worker-b").unwrap()[0], other);

    reopened
        .acknowledge(StableOutboundAcknowledgement {
            destination_node: "worker-a".to_owned(),
            sequence: first.sequence,
            lease_term: first.lease_term,
            fencing_token: first.fencing_token,
            record_digest: first.record_digest,
        })
        .unwrap();
    assert_eq!(reopened.pending("worker-a").unwrap(), vec![second.clone()]);
    // The same acknowledgement is safe to retry after a response loss.
    reopened
        .acknowledge(StableOutboundAcknowledgement {
            destination_node: "worker-a".to_owned(),
            sequence: first.sequence,
            lease_term: first.lease_term,
            fencing_token: first.fencing_token,
            record_digest: first.record_digest,
        })
        .unwrap();

    let mut after_restart = StableOutboundLog::open(&path, brain, 8).unwrap();
    assert_eq!(after_restart.pending("worker-a").unwrap(), vec![second]);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn reappending_an_identical_pending_message_reuses_its_sealed_sequence() {
    let path = temporary_path("idempotent-append");
    let brain = BrainId::new(80).unwrap();
    let mut log = StableOutboundLog::open(&path, brain, 1).unwrap();
    let message = message(20, 1);
    let first = log
        .append("worker-a", LeaseTerm::INITIAL, 1, message.clone())
        .unwrap();
    let retry = log
        .append("worker-a", LeaseTerm::INITIAL, 1, message)
        .unwrap();
    assert_eq!(retry, first);
    assert_eq!(log.pending("worker-a").unwrap(), vec![first]);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn fencing_and_conflicting_acknowledgements_fail_closed() {
    let path = temporary_path("fence");
    let brain = BrainId::new(78).unwrap();
    let mut log = StableOutboundLog::open(&path, brain, 2).unwrap();
    let old = log
        .append("worker", LeaseTerm::INITIAL, 1, message(10, 1))
        .unwrap();
    log.fence("worker", LeaseTerm::new(2).unwrap(), 2).unwrap();

    assert!(matches!(
        log.append("worker", LeaseTerm::INITIAL, 1, message(11, 1)),
        Err(StableOutboundError::StaleAuthority { .. })
    ));
    assert!(matches!(
        log.acknowledge(StableOutboundAcknowledgement {
            destination_node: "worker".to_owned(),
            sequence: old.sequence,
            lease_term: old.lease_term,
            fencing_token: old.fencing_token,
            record_digest: old.record_digest,
        }),
        Err(StableOutboundError::StaleAuthority { .. })
    ));
    assert!(matches!(
        log.acknowledge(StableOutboundAcknowledgement {
            destination_node: "worker".to_owned(),
            sequence: old.sequence,
            lease_term: LeaseTerm::new(2).unwrap(),
            fencing_token: 2,
            record_digest: StateDigest([9; 16]),
        }),
        Err(StableOutboundError::AcknowledgementMismatch)
    ));
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn corrupted_storage_is_rejected_on_restart() {
    let path = temporary_path("corrupt");
    std::fs::write(&path, b"not-json").unwrap();
    assert!(matches!(
        StableOutboundLog::open(&path, BrainId::new(79).unwrap(), 4),
        Err(StableOutboundError::Corrupt(_))
    ));
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
