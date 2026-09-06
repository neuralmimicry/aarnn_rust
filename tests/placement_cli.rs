//! Black-box coverage for the offline placement proposal CLI.
//!
//! The test deliberately invokes the compiled binary. This verifies the
//! serialised request boundary and proves that the local QA path returns the
//! same verified, non-applied result promised by the orchestrator adapter.

use aarnn_rust::deterministic::{BrainId, LeaseTerm, LogicalTag, ShardId};
use aarnn_rust::placement::{
    PlacementConstraints, PlacementIntent, PlacementRequest, ResourceObservation, ShardDemand,
};
use std::fs;
use std::process::Command;

use aarnn_rust::migration_operation::{
    MigrationCancellation, MigrationKind, MigrationPhase, MigrationProgress, MigrationRequest,
    MigrationTransition,
};

fn request() -> PlacementRequest {
    PlacementRequest {
        brain_id: BrainId::new(42).unwrap(),
        topology_generation: aarnn_rust::deterministic::TopologyGeneration::INITIAL,
        partition_generation: aarnn_rust::deterministic::PartitionGeneration::INITIAL,
        lease_term: LeaseTerm::INITIAL,
        fencing_token: LeaseTerm::INITIAL.raw(),
        effective_tag: LogicalTag::ZERO,
        demands: vec![ShardDemand {
            shard_id: ShardId::new(1).unwrap(),
            load_units: 10,
            memory_bytes: 100,
            checkpoint_bytes: 100,
            network_bytes_per_second: 10,
            zero_delay_component: None,
            required_numerical_profile: "reference-cpu-v1".to_owned(),
            preferred_node: None,
        }],
        resources: vec![ResourceObservation {
            node_id: "laptop".to_owned(),
            device_id: "laptop-cpu".to_owned(),
            healthy: true,
            enrolled: true,
            compute_authorised: true,
            failure_domain: "host:laptop".to_owned(),
            numerical_profiles: vec!["reference-cpu-v1".to_owned()],
            capacity_units: 100,
            reserved_capacity_units: 0,
            memory_bytes: 1_000,
            reserved_memory_bytes: 0,
            storage_bytes: 1_000,
            reserved_storage_bytes: 0,
            network_bytes_per_second: 1_000,
            reserved_network_bytes_per_second: 0,
            cpu_pressure_milli: 100,
            memory_pressure_milli: 100,
            network_pressure_milli: 100,
            thermal_pressure_milli: 100,
        }],
        constraints: PlacementConstraints {
            minimum_warm_replicas: 0,
            ..PlacementConstraints::default()
        },
        intent: PlacementIntent::Automatic,
    }
}

#[test]
fn local_cli_returns_verified_proposal_without_applying_it() {
    let path = std::env::temp_dir().join(format!(
        "aarnn-placement-cli-{}-{}.json",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    fs::write(&path, serde_json::to_vec(&request()).unwrap()).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_aarnn_rust"))
        .args([
            "--placement-request-local-json",
            path.to_str().expect("temporary path is UTF-8"),
        ])
        .output()
        .expect("local placement CLI should start");
    let _ = fs::remove_file(&path);

    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["transport"], "local-reference");
    assert_eq!(result["applied"], false);
    assert_eq!(result["plan"]["placements"].as_array().unwrap().len(), 1);
    assert_eq!(result["plan"]["placements"][0]["active_node"], "laptop");
    assert!(!result["command_digest"].as_array().unwrap().is_empty());
}

#[test]
fn brain_placement_plan_alias_keeps_the_proposal_path_readable() {
    let path = std::env::temp_dir().join(format!(
        "aarnn-placement-alias-{}-{}.json",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    fs::write(&path, serde_json::to_vec(&request()).unwrap()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_aarnn_rust"))
        .args(["--brain-placement-plan", path.to_str().unwrap()])
        .output()
        .unwrap();
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "CLI alias failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["applied"], false);
}

#[test]
fn structured_brain_placement_plan_uses_the_same_verified_handler() {
    let path = std::env::temp_dir().join(format!(
        "aarnn-placement-command-{}-{}.json",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    fs::write(&path, serde_json::to_vec(&request()).unwrap()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_aarnn_rust"))
        .args(["brain", "placement", "plan", path.to_str().unwrap()])
        .output()
        .unwrap();
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "structured placement command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["transport"], "local-reference");
    assert_eq!(result["applied"], false);
}

#[test]
fn local_cli_persists_and_advances_a_fenced_migration_operation() {
    let root = std::env::temp_dir().join(format!(
        "aarnn-migration-cli-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let journal = root.join("migration.json");
    let request_path = root.join("request.json");
    let transition_path = root.join("transition.json");
    let request = MigrationRequest {
        request_id: "cli-migration-request".to_owned(),
        idempotency_key: "cli-migration-key".to_owned(),
        brain_id: BrainId::new(42).unwrap(),
        observed_leader_term: LeaseTerm::INITIAL,
        expected_resource_version: 0,
        kind: MigrationKind::Consolidate,
        source_plan_digest: aarnn_rust::deterministic::StateDigest([1; 16]),
        target_plan_digest: aarnn_rust::deterministic::StateDigest([2; 16]),
        total_shards: 1,
        total_bytes: 10,
    };
    fs::write(&request_path, serde_json::to_vec(&request).unwrap()).unwrap();
    let submit = Command::new(env!("CARGO_BIN_EXE_aarnn_rust"))
        .args([
            "--migration-submit-local-json",
            request_path.to_str().unwrap(),
            "--migration-journal-json",
            journal.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        submit.status.success(),
        "migration submit failed: {}",
        String::from_utf8_lossy(&submit.stderr)
    );
    let submitted: serde_json::Value = serde_json::from_slice(&submit.stdout).unwrap();
    assert_eq!(submitted["operation"]["phase"], "Prepared");
    assert_eq!(submitted["resource_version"], 1);

    let transition = MigrationTransition {
        operation_id: 1,
        observed_leader_term: LeaseTerm::INITIAL,
        expected_resource_version: 1,
        next_phase: MigrationPhase::Reserving,
        progress: MigrationProgress::new(1, 10).unwrap(),
        error_code: None,
    };
    fs::write(&transition_path, serde_json::to_vec(&transition).unwrap()).unwrap();
    let advanced = Command::new(env!("CARGO_BIN_EXE_aarnn_rust"))
        .args([
            "--migration-transition-local-json",
            transition_path.to_str().unwrap(),
            "--migration-journal-json",
            journal.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        advanced.status.success(),
        "migration transition failed: {}",
        String::from_utf8_lossy(&advanced.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&advanced.stdout).unwrap();
    assert_eq!(result["operation"]["phase"], "Reserving");
    assert_eq!(result["resource_version"], 2);

    let cancellation_path = root.join("cancellation.json");
    fs::write(
        &cancellation_path,
        serde_json::to_vec(&MigrationCancellation {
            operation_id: 1,
            observed_leader_term: LeaseTerm::INITIAL,
            expected_resource_version: 2,
            reason: "laptop is leaving the network".to_owned(),
        })
        .unwrap(),
    )
    .unwrap();
    let cancelled = Command::new(env!("CARGO_BIN_EXE_aarnn_rust"))
        .args([
            "--migration-cancel-local-json",
            cancellation_path.to_str().unwrap(),
            "--migration-journal-json",
            journal.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        cancelled.status.success(),
        "migration cancellation failed: {}",
        String::from_utf8_lossy(&cancelled.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&cancelled.stdout).unwrap();
    assert_eq!(result["operation"]["phase"], "Aborted");
    assert_eq!(result["resource_version"], 4);

    let watched = Command::new(env!("CARGO_BIN_EXE_aarnn_rust"))
        .args([
            "--operation-watch",
            journal.to_str().unwrap(),
            "--operation-watch-id",
            "1",
        ])
        .output()
        .unwrap();
    assert!(
        watched.status.success(),
        "operation watch failed: {}",
        String::from_utf8_lossy(&watched.stderr)
    );
    let watched: serde_json::Value = serde_json::from_slice(&watched.stdout).unwrap();
    assert_eq!(watched["transport"], "local-reference-read-only");
    assert_eq!(watched["operations"]["phase"], "Aborted");
    assert!(journal.exists());
    let _ = fs::remove_dir_all(root);
}
