use aarnn_biox6_exporter::{
    build_plan, export_artifacts, export_bundle, load_calibration, load_machine, load_snapshot,
    preview_html_with_defaults, snapshot_from_json_str, zip_export_with_defaults,
};
use std::path::PathBuf;

fn repo(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

#[test]
fn example_exports() {
    let snapshot = load_snapshot(&repo("examples/aarnn_snapshot.json")).unwrap();
    let machine = load_machine(&repo("config/machine.example.yaml")).unwrap();
    let calibration = load_calibration(&repo("config/calibration.example.yaml")).unwrap();
    let plan = build_plan(&snapshot, &machine, &calibration).unwrap();
    let bundle = export_bundle(&plan, &machine, &calibration).unwrap();
    assert_eq!(plan.nodes.len(), 3);
    assert!(bundle.gcode.contains("AARNN network"));
    let artifacts = export_artifacts(&bundle, &machine, &calibration).unwrap();
    assert!(artifacts
        .iter()
        .any(|a| a.relative_path == "preview/preview.html"));
    assert!(artifacts
        .iter()
        .any(|a| a.relative_path == "verification.json"));
    let preview_html = artifacts
        .iter()
        .find(|a| a.relative_path == "preview/preview.html")
        .and_then(|a| std::str::from_utf8(&a.bytes).ok())
        .unwrap();
    assert!(preview_html.contains(r#"id="preview-svg""#));
    assert!(preview_html.contains("Wheel zoom. Drag to pan."));
    assert!(preview_html.contains("Shift-drag, right-drag, or middle-drag"));

    let raw = std::fs::read_to_string(repo("examples/aarnn_snapshot.json")).unwrap();
    let (preview, preview_plan) = preview_html_with_defaults(&raw, Some("helper-demo")).unwrap();
    assert_eq!(preview_plan.source_network_id, "demo-aarnn");
    assert!(preview.contains(r#"id="preview-svg""#));
    let (zip, zip_bundle) = zip_export_with_defaults(&raw, Some("helper-demo")).unwrap();
    assert_eq!(zip_bundle.plan.nodes.len(), 3);
    assert!(zip.starts_with(b"PK"));
}

#[test]
fn runner_snapshot_adapter_exports() {
    let raw = r#"{
      "net": {
        "num_sensory_neurons": 1,
        "num_hidden_layers": 1,
        "num_hidden_per_layer_initial": 1,
        "num_output_neurons": 1,
        "use_aarnn_delays": true,
        "aarnn_velocity": 10.0,
        "bouton_latency_ms": 0.5
      },
      "t": 42,
      "t_ms": 42.0,
      "w_in": { "rows": 1, "cols": 1, "data": [0.7] },
      "w_hh_fwd": [],
      "w_hh_bwd": [],
      "w_hh_rec": [
        { "rows": 1, "cols": 1, "data": [0.0] }
      ],
      "w_out": { "rows": 1, "cols": 1, "data": [-0.3] },
      "p_in": { "rows": 1, "cols": 1, "data": [1] },
      "p_rec": [],
      "p_out": { "rows": 1, "cols": 1, "data": [1] }
    }"#;
    let snapshot = snapshot_from_json_str(raw, Some("runner-demo")).unwrap();
    assert_eq!(snapshot.network_id, "runner-demo");
    assert_eq!(snapshot.nodes.len(), 3);
    assert_eq!(snapshot.connections.len(), 2);
    assert!(snapshot.connections.iter().any(|c| c.weight < 0.0));
}

#[test]
fn parallel_synapses_between_same_nodes_get_separate_routes() {
    let raw = r#"{
      "schema_version": "0.1",
      "network_id": "parallel-demo",
      "captured_at_tick": 1,
      "time_unit_us": 1.0,
      "nodes": [
        {
          "id": "hidden-0-97",
          "kind": "interneuron",
          "threshold": 0.5,
          "refractory_us": 1000.0,
          "preferred_position_mm": { "x": 20.0, "y": 30.0, "z": 3.0 }
        },
        {
          "id": "output-0",
          "kind": "readout",
          "threshold": 0.6,
          "refractory_us": 1000.0,
          "preferred_position_mm": { "x": 105.0, "y": 55.0, "z": 3.0 }
        }
      ],
      "connections": [
        {
          "id": "synapse:morph:0:hidden-0-97:output-0",
          "source_node": "hidden-0-97",
          "target_node": "output-0",
          "weight": 0.8,
          "delay_us": 500.0,
          "enabled": true
        },
        {
          "id": "synapse:morph:1:hidden-0-97:output-0",
          "source_node": "hidden-0-97",
          "target_node": "output-0",
          "weight": 0.6,
          "delay_us": 700.0,
          "enabled": true
        }
      ],
      "metadata": {}
    }"#;
    let snapshot = snapshot_from_json_str(raw, Some("parallel-demo")).unwrap();
    let machine = load_machine(&repo("config/machine.example.yaml")).unwrap();
    let calibration = load_calibration(&repo("config/calibration.example.yaml")).unwrap();
    let plan = build_plan(&snapshot, &machine, &calibration).unwrap();
    assert_eq!(plan.connections.len(), 2);
}

#[test]
#[ignore = "uses the larger repository-level network_celegans.json fixture; run explicitly"]
fn celegans_dense_fixture_exports_best_effort_with_warnings() {
    let raw = std::fs::read_to_string(repo("../network_celegans.json")).unwrap();

    let (preview, preview_plan) =
        preview_html_with_defaults(&raw, Some("network_celegans")).unwrap();
    assert!(preview.contains(r#"id="preview-svg""#));
    assert_best_effort_warning_present(&preview_plan.warnings);

    let (zip, zip_bundle) = zip_export_with_defaults(&raw, Some("network_celegans")).unwrap();
    assert!(zip.starts_with(b"PK"));
    assert_best_effort_warning_present(&zip_bundle.plan.warnings);
}

fn assert_best_effort_warning_present(warnings: &[aarnn_biox6_exporter::model::PlanWarning]) {
    assert!(
        warnings.iter().any(|warning| {
            matches!(
                warning.code.as_str(),
                "PORT_BEST_EFFORT_OVERLAP"
                    | "PORT_DENSE_ALLOCATION_BEST_EFFORT"
                    | "ROUTE_BEST_EFFORT_OVERLAP"
                    | "ROUTE_BEST_EFFORT_WARNINGS_TRUNCATED"
                    | "DENSE_ROUTING_DETERMINISTIC_BEST_EFFORT"
                    | "ROUTING_COMPLETE_BEST_EFFORT"
                    | "ROUTING_GRID_DEGRADED_BEST_EFFORT"
                    | "ROUTING_LANES_REUSED_BEST_EFFORT"
                    | "PHYSICAL_VALIDATION_EXHAUSTIVE_SKIPPED_BEST_EFFORT"
                    | "PHYSICAL_VALIDATION_FAILED_BEST_EFFORT"
            )
        }),
        "{warnings:#?}"
    );
}
