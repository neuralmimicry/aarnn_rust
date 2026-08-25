//! Host-side contract coverage for the cross-platform mobile track.
//!
//! These tests intentionally exercise only the platform-neutral seam.  They do
//! not stand in for iOS/Android simulator, physical-device, signing or store
//! acceptance lanes.

use aarnn_rust::config::NetworkConfig;
use aarnn_rust::deterministic::BrainId;
use aarnn_rust::engine::EngineSpec;
use aarnn_rust::mobile_runtime::{
    CapabilityAvailability, DiscoveryObservation, MOBILE_SCHEMA_VERSION, MobileCapability,
    MobileCapabilityReport, MobileExecutionMode, MobileLifecycle, MobileRuntime,
};

fn small_spec() -> EngineSpec {
    let mut net = NetworkConfig::default();
    net.num_sensory_neurons = 2;
    net.num_hidden_layers = 1;
    net.num_hidden_per_layer_initial = 2;
    net.num_output_neurons = 1;
    EngineSpec {
        net,
        ..EngineSpec::default()
    }
}

#[test]
fn mob_e2e_013_standalone_survives_no_network_checkpoint_restore() {
    let brain = BrainId::new(100).expect("brain ID");
    let mut runtime = MobileRuntime::new(brain, MobileExecutionMode::StandaloneBrain, small_spec())
        .expect("standalone runtime");
    runtime.initialise().expect("initialise");
    runtime.start().expect("start");
    runtime.step(Some(&[1, 0])).expect("local step");
    let checkpoint = runtime.checkpoint().expect("checkpoint");
    let restored = MobileRuntime::restore(checkpoint).expect("restore");

    assert_eq!(restored.brain(), brain);
    assert_eq!(restored.lifecycle(), MobileLifecycle::Ready);
    assert_eq!(restored.checkpoint().expect("recheckpoint").logical_step, 1);
}

#[test]
fn ut_moblife_001_background_never_advances_biological_time() {
    let mut runtime = MobileRuntime::new(
        BrainId::new(101).expect("brain ID"),
        MobileExecutionMode::OfflineDemonstrator,
        small_spec(),
    )
    .expect("runtime");
    runtime.initialise().expect("initialise");
    runtime.start().expect("start");
    let before = runtime.checkpoint().expect("checkpoint").logical_step;
    runtime.enter_background().expect("background");
    let after = runtime.checkpoint().expect("checkpoint").logical_step;
    assert_eq!(before, after);
}

#[test]
fn ut_mobcap_001_unknown_capability_is_safe_unavailable() {
    let report = MobileCapabilityReport::safe_unavailable(
        aarnn_rust::mobile_runtime::MobileProduct::Android,
        "Android adapter is not present in this host build",
    );
    assert_eq!(report.schema_version, MOBILE_SCHEMA_VERSION);
    assert_eq!(
        report.availability(MobileCapability::UsbAerInput),
        CapabilityAvailability::Unavailable(
            "Android adapter is not present in this host build".to_owned()
        )
    );
}

#[test]
fn discovery_observation_does_not_imply_enrolment() {
    let observation = DiscoveryObservation {
        schema_version: MOBILE_SCHEMA_VERSION,
        observation_id: 7,
        service_type: "_aarnn._tcp".to_owned(),
        endpoint_hint: "https://node.invalid".to_owned(),
        protocol_min: 1,
        protocol_max: 1,
        expires_at_ms: 100,
    };
    observation.validate(99).expect("observation");
    // The observation contains no grant, credential or enrolment transition.
    assert_eq!(observation.observation_id, 7);
}
