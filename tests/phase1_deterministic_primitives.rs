use aarnn_rust::deterministic::{
    BrainId, CanonicalEvent, CanonicalEventKey, CounterRng, EventId, EventStage, LogicalTag,
    PrimitiveError, Q32_32, RngCoordinate, StableDenseMap, StateDigestBuilder, TopologyGeneration,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct PrimitiveFixture {
    schema_version: u16,
    brain_id: u64,
    topology_generation: u64,
    logical_tag: LogicalTag,
    q32_32_raw: i64,
    event_digest: String,
    state_digest: String,
}

fn events_in_order() -> Vec<CanonicalEvent> {
    vec![
        CanonicalEvent {
            key: CanonicalEventKey::new(
                LogicalTag::new(12, 3),
                EventStage::PostsynapticEffect,
                9,
                4,
                2,
            ),
            payload: vec![3, 1],
        },
        CanonicalEvent {
            key: CanonicalEventKey::new(
                LogicalTag::new(12, 2),
                EventStage::SynapticTransition,
                7,
                4,
                1,
            ),
            payload: vec![2, 8],
        },
    ]
}

#[test]
fn phase1_ids_time_numeric_and_golden_serialisation_are_stable() {
    let fixture: PrimitiveFixture =
        serde_json::from_str(include_str!("fixtures/phase1_primitives.json"))
            .expect("valid Phase 1 primitive fixture");
    let brain = BrainId::new(fixture.brain_id).unwrap();
    assert_eq!(fixture.schema_version, 1);
    assert_eq!(brain.raw(), 7);
    assert_eq!(
        TopologyGeneration::new(fixture.topology_generation)
            .unwrap()
            .raw(),
        2
    );
    assert_eq!(fixture.logical_tag, LogicalTag::new(12, 3));
    assert_eq!(Q32_32::from_ratio(3, 2).unwrap().raw(), fixture.q32_32_raw);

    let map = StableDenseMap::new(
        TopologyGeneration::new(2).unwrap(),
        vec![BrainId::new(7).unwrap(), BrainId::new(11).unwrap()],
    )
    .unwrap();
    assert_eq!(map.stable_to_dense(brain), Some(0));
    assert_eq!(map.dense_to_stable(1).unwrap().raw(), 11);
    assert!(matches!(BrainId::new(0), Err(PrimitiveError::ZeroId)));
    assert!(
        map.require_generation(TopologyGeneration::new(3).unwrap())
            .is_err()
    );

    let event_digest = aarnn_rust::deterministic::canonical_event_digest(&events_in_order());
    let reversed = events_in_order().into_iter().rev().collect::<Vec<_>>();
    assert_eq!(
        event_digest,
        aarnn_rust::deterministic::canonical_event_digest(&reversed)
    );

    let mut state = StateDigestBuilder::default();
    state.add_domain("zeta", b"two");
    state.add_domain("alpha", b"one");
    let state_digest = state.finish();
    assert_eq!(event_digest.to_string(), fixture.event_digest);
    assert_eq!(state_digest.to_string(), fixture.state_digest);
}

#[test]
fn phase1_logical_time_boundaries_are_checked_before_mutation() {
    let tag = LogicalTag::new(8, 2);
    assert_eq!(tag.advance(0).unwrap(), LogicalTag::new(8, 3));
    assert_eq!(tag.advance(4).unwrap(), LogicalTag::new(12, 0));
    assert_eq!(
        LogicalTag::new(u64::MAX, 0).next_quantum(),
        Err(PrimitiveError::LogicalTimeOverflow {
            operation: "deferring to the next quantum"
        })
    );
    assert_eq!(
        LogicalTag::new(2, 0)
            .ensure_not_before(LogicalTag::new(3, 0))
            .unwrap_err(),
        PrimitiveError::BackwardsTag {
            current: LogicalTag::new(3, 0),
            next: LogicalTag::new(2, 0)
        }
    );
}

#[test]
fn phase1_rng_is_coordinate_addressed_not_traversal_addressed() {
    let rng = CounterRng::new(42, 9);
    let first = RngCoordinate {
        brain: BrainId::new(7).unwrap(),
        entity: 10,
        event: EventId::new(3).unwrap(),
        purpose: 4,
        draw: 0,
    };
    let second = RngCoordinate {
        entity: 11,
        ..first
    };
    let forward = [first, second]
        .into_iter()
        .map(|coordinate| rng.draw_u64(coordinate))
        .collect::<Vec<_>>();
    let reverse = [second, first]
        .into_iter()
        .map(|coordinate| rng.draw_u64(coordinate))
        .collect::<Vec<_>>();
    assert_eq!(forward[0], reverse[1]);
    assert_eq!(forward[1], reverse[0]);
    assert_ne!(forward[0], forward[1]);
    assert!(rng.uniform01(first) < 1.0);
}

#[test]
fn phase1_versioned_nonzero_wrappers_reject_zero_on_deserialisation() {
    assert!(serde_json::from_str::<TopologyGeneration>("0").is_err());
    assert!(serde_json::from_str::<aarnn_rust::deterministic::PartitionGeneration>("0").is_err());
    assert!(serde_json::from_str::<aarnn_rust::deterministic::LeaseTerm>("0").is_err());
    assert!(serde_json::from_str::<aarnn_rust::deterministic::SchemaVersion>("0").is_err());
    assert!(serde_json::from_str::<BrainId>("0").is_err());
}
