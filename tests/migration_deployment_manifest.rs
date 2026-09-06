//! Deployment manifest validation for automatic migration executor startup.
//!
//! These tests cover the configuration boundary that turns deployment-owned
//! JSON into live migration settings.  They do not claim a physical cutover;
//! the executor and remote activation tests cover that separately.

#![cfg(feature = "stable_executor_live")]

use aarnn_rust::consistent_cut::{ChannelMarker, ConsistentCutCoordinator, ParticipantReport};
use aarnn_rust::deterministic::{LeaseTerm, LogicalTag, ShardId};
use aarnn_rust::managed_durability::managed_brain_id;
use aarnn_rust::migration_executor::{
    STABLE_MIGRATION_DEPLOYMENT_SCHEMA_VERSION, StableMigrationDeploymentManifest,
};
use aarnn_rust::placement::{
    PlacementConstraints, PlacementIntent, PlacementPlanner, PlacementRequest, ResourceObservation,
    ShardDemand,
};
use aarnn_rust::stable_worker::StableWorkerActivationCommand;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn cut() -> aarnn_rust::consistent_cut::ConsistentCut {
    let mut coordinator =
        ConsistentCutCoordinator::begin(1, ["source".to_owned()], ["source->target".to_owned()])
            .unwrap();
    coordinator
        .record_report(ParticipantReport {
            participant: "source".to_owned(),
            local_frontier: LogicalTag::ZERO,
            queued_min: None,
            in_flight_min: None,
            activity_epoch: 1,
        })
        .unwrap();
    coordinator
        .record_marker(ChannelMarker::new("source->target", 1, None, b"empty").unwrap())
        .unwrap();
    coordinator.finalise().unwrap()
}

fn resource() -> ResourceObservation {
    ResourceObservation {
        node_id: "target".to_owned(),
        device_id: "target-cpu".to_owned(),
        healthy: true,
        enrolled: true,
        compute_authorised: true,
        failure_domain: "rack-target".to_owned(),
        numerical_profiles: vec!["reference-cpu-v1".to_owned()],
        capacity_units: 100,
        reserved_capacity_units: 0,
        memory_bytes: 10_000,
        reserved_memory_bytes: 0,
        storage_bytes: 10_000,
        reserved_storage_bytes: 0,
        network_bytes_per_second: 10_000,
        reserved_network_bytes_per_second: 0,
        cpu_pressure_milli: 100,
        memory_pressure_milli: 100,
        network_pressure_milli: 100,
        thermal_pressure_milli: 100,
    }
}

fn manifest(root: PathBuf) -> StableMigrationDeploymentManifest {
    let brain = managed_brain_id("deployment-manifest-network");
    let plan = PlacementPlanner
        .plan(PlacementRequest {
            brain_id: brain,
            topology_generation: aarnn_rust::deterministic::TopologyGeneration::INITIAL,
            partition_generation: aarnn_rust::deterministic::PartitionGeneration::INITIAL,
            lease_term: LeaseTerm::new(2).unwrap(),
            fencing_token: 2,
            effective_tag: LogicalTag::ZERO,
            demands: vec![ShardDemand {
                shard_id: ShardId::new(10).unwrap(),
                load_units: 1,
                memory_bytes: 1,
                checkpoint_bytes: 1,
                network_bytes_per_second: 1,
                zero_delay_component: None,
                required_numerical_profile: "reference-cpu-v1".to_owned(),
                preferred_node: Some("target".to_owned()),
            }],
            resources: vec![resource()],
            constraints: PlacementConstraints {
                minimum_warm_replicas: 0,
                ..PlacementConstraints::default()
            },
            intent: PlacementIntent::Consolidate {
                target_node: "target".to_owned(),
            },
        })
        .unwrap();
    let command = StableWorkerActivationCommand::new(
        "deployment-activation",
        1,
        brain.raw(),
        "deployment-manifest-network",
        "target",
        "{}",
    )
    .unwrap();
    StableMigrationDeploymentManifest {
        schema_version: STABLE_MIGRATION_DEPLOYMENT_SCHEMA_VERSION,
        network_id: "deployment-manifest-network".to_owned(),
        source_node: "source".to_owned(),
        consistent_cut: cut(),
        destination_root: root.join("destination"),
        warm_root: root.join("warm"),
        authority_replica_paths: [
            ("source".to_owned(), root.join("authority-source.json")),
            ("target".to_owned(), root.join("authority-target.json")),
            ("witness".to_owned(), root.join("authority-witness.json")),
        ]
        .into_iter()
        .collect(),
        authority_members: ["source", "target", "witness"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        destination_nodes: [(ShardId::new(10).unwrap(), "target".to_owned())]
            .into_iter()
            .collect(),
        source_fencing_tokens: [(ShardId::new(10).unwrap(), 1)].into_iter().collect(),
        placement_registry_path: root.join("placement.json"),
        target_plan: plan,
        stream_id: aarnn_rust::deterministic::StreamId::new(7).unwrap(),
        max_payload: 1024,
        frame_bytes: 128,
        destination_endpoints: [(
            "target".to_owned(),
            "https://target.example:50051".to_owned(),
        )]
        .into_iter()
        .collect(),
        target_activation_commands: [("target".to_owned(), command)].into_iter().collect(),
    }
}

#[test]
fn deployment_manifest_validates_and_constructs_persistent_settings() {
    let root = std::env::temp_dir().join(format!(
        "aarnn-migration-deployment-manifest-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let manifest = manifest(root.clone());
    let settings = manifest
        .clone()
        .validate()
        .unwrap()
        .into_settings()
        .unwrap();
    assert_eq!(settings.source_node, "source");
    assert_eq!(settings.destination_endpoints.len(), 1);
    assert_eq!(settings.target_activation_commands.len(), 1);
    assert!(root.join("placement.json").exists());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn deployment_manifest_rejects_target_set_mismatch_before_opening_files() {
    let root = std::env::temp_dir().join(format!(
        "aarnn-migration-deployment-invalid-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let mut manifest = manifest(root.clone());
    manifest.destination_endpoints = BTreeMap::new();
    let error = manifest.validate().unwrap_err();
    assert!(error.contains("destination endpoints"));
    assert!(
        !root.exists(),
        "validation must precede filesystem creation"
    );
}

#[test]
fn deployment_manifest_load_is_bounded_and_versioned() {
    let root = std::env::temp_dir().join(format!(
        "aarnn-migration-deployment-load-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("migration.json");
    let mut value = serde_json::to_value(manifest(root.clone())).unwrap();
    value["schema_version"] = serde_json::json!(999);
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    let error = StableMigrationDeploymentManifest::load(&path).unwrap_err();
    assert!(error.contains("unsupported stable migration manifest schema"));
    let _ = std::fs::remove_dir_all(root);
}
