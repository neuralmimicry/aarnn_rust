use std::fs;
use std::path::PathBuf;

fn launcher() -> String {
    fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/run_examples.sh"))
        .expect("run_examples.sh must be present")
}

#[test]
fn example_launcher_uses_the_local_non_management_profile() {
    let source = launcher();
    assert!(
        source.contains("SCRIPT_DIR=\"$(cd -- \"$(dirname -- \"${BASH_SOURCE[0]}\")\" && pwd)\""),
        "the launcher must anchor relative paths to its own checkout"
    );
    assert!(
        source.contains("cargo build --release --locked --no-default-features \\\n        --bin aarnn_rust --bin web_ui"),
        "the example launcher must build the local orchestrator profile explicitly"
    );
    assert!(
        source.contains("--features \"engine_runtime,ui,cuda\""),
        "the native example must build both OpenCL and CUDA candidates for latency selection"
    );
    assert!(
        source.contains("--advertise-addr \"127.0.0.1:$NODE1_PORT\"")
            && source.contains("--advertise-addr \"127.0.0.1:$NODE2_PORT\""),
        "local nodes must advertise reachable loopback endpoints rather than wildcard bind addresses"
    );
    assert!(
        !source.contains("cargo build --release --all-features"),
        "examples must not inherit the authenticated production management service"
    );
    assert!(source.contains("--execution-mode distributed,sharded"));
    assert!(source.contains("--execution-scope cluster"));
    assert!(source.contains("--execution-desired-shards 2"));
    assert!(source.contains("NM_DISTRIBUTED_AUTOSTART=1"));
}

#[test]
fn example_launcher_reports_a_ready_dashboard_url() {
    let source = launcher();
    assert!(source.contains("WEB_UI_URL=\"http://127.0.0.1:$WEB_UI_PORT\""));
    assert!(
        source.contains("$WEB_UI_URL/api/config"),
        "the launcher must verify the dashboard before reporting its URL"
    );
    assert!(
        source.contains("echo \"Web dashboard URL (port $WEB_UI_PORT): $WEB_UI_URL\""),
        "the launcher must print the exact dashboard URL and port users can open"
    );
}

#[test]
fn example_launcher_has_an_opt_in_growth_and_relocation_probe() {
    let source = launcher();
    assert!(source.contains("AARNN_VERIFY_SHARD_GROWTH"));
    assert!(source.contains("AARNN_VERIFY_HIERARCHICAL_SHARDING"));
    assert!(source.contains("verify_hierarchical_sharding.py"));
    assert!(source.contains("Live area/layer/sub-shard placement telemetry verified"));
    assert!(source.contains("verify_example_sharding.py"));
    assert!(source.contains("--make-fixture"));
    assert!(source.contains("--node1-pid \"$NODE1_PID\""));
    assert!(source.contains("--node2-pid \"$NODE2_PID\""));
    assert!(source.contains("AARNN_ORCH_PORT_START"));
    assert!(source.contains("AARNN_NODE1_PORT_START"));
    assert!(source.contains("AARNN_NODE2_PORT_START"));
    assert!(source.contains("NM_DISCOVERY_TARGETS"));
    assert!(source.contains("NM_DISCOVERY_DISABLE_DEFAULTS"));
}

#[test]
fn example_launcher_validates_and_forwards_explicit_audio_input() {
    let source = launcher();
    assert!(source.contains("AARNN_AUDIO_FILE"));
    assert!(source.contains("AARNN_AUDIO_SENSORY_NEURONS"));
    assert!(source.contains("not a readable regular file"));
    assert!(source.contains("export AARNN_AUDIO_FILE AARNN_AUDIO_SENSORY_NEURONS"));
    assert!(source.contains("must be at most 65536"));
}

#[test]
fn webcluster_launcher_uses_the_same_local_profile_and_dashboard_output() {
    let source = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/run_webcluster.sh"))
        .expect("run_webcluster.sh must be present");
    assert!(
        source.contains("cargo build --release --locked --no-default-features \\\n        --bin aarnn_rust --bin web_ui"),
        "the webcluster launcher must not compile the authenticated all-features profile"
    );
    assert!(
        source.contains("--features \"engine_runtime,ui,cuda\""),
        "the webcluster launcher must build both GPU candidates for latency selection"
    );
    assert!(
        source.contains("--advertise-addr \"127.0.0.1:$NODE1_PORT\"")
            && source.contains("--advertise-addr \"127.0.0.1:$NODE2_PORT\""),
        "local nodes must advertise reachable loopback endpoints rather than wildcard bind addresses"
    );
    assert!(
        !source.contains("cargo build --release --all-features"),
        "the webcluster launcher must not inherit the authenticated management service"
    );
    assert!(
        source.contains("$WEB_UI_URL/api/config"),
        "the webcluster launcher must verify the dashboard before reporting its URL"
    );
    assert!(
        source.contains("echo \"Web dashboard URL (port $WEB_UI_PORT): $WEB_UI_URL\""),
        "the webcluster launcher must print the exact dashboard URL and port"
    );
}

#[test]
fn parameterized_cluster_launcher_supports_standalone_single_and_multi_worker_modes() {
    let source = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/scripts/run_cluster.sh"
    ))
    .expect("scripts/run_cluster.sh must be present");
    assert!(source.contains("--standalone"));
    assert!(source.contains("--nodes COUNT"));
    assert!(source.contains("COUNT=1 is a single-worker cluster"));
    assert!(source.contains("COUNT>=2 is"));
    assert!(source.contains("--no-web"));
    assert!(source.contains("wait_for_port"));
    assert!(source.contains("if ! kill -0 \"$pid\""));
    assert!(source.contains("reserve_explicit_port"));
    assert!(source.contains("--orchestrator-addr \"http://127.0.0.1:$ORCH_PORT\""));
    assert!(source.contains("--execution-mode distributed,sharded"));
    assert!(source.contains("--execution-scope cluster"));
    assert!(source.contains("--execution-desired-shards"));
    assert!(source.contains("NM_DISTRIBUTE_STARTUP_SNAPSHOT=1"));
}

#[test]
fn webots_launcher_forwards_and_materializes_requested_cluster_workers() {
    let source = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/run_webot.sh"))
        .expect("run_webot.sh must be present");
    assert!(source.contains("--nodes <n>"));
    assert!(source.contains("NODE_COUNT=\"${NM_CLUSTER_NODES:-1}\""));
    assert!(source.contains("NODE_COUNT=\"${1:-}\""));
    assert!(
        source.contains("worker_id=\"${worker_brain}_worker_$(printf '%02d' \"$extra_index\")\"")
    );
    assert!(source.contains("Successfully joined orchestrator"));
    assert!(source.contains("worker processes: $NODE_COUNT"));
    assert!(source.contains("engine_runtime,ui,robot_io,cuda"));
    assert!(source.contains("spec[\"execution_modes\"] = [\"distributed\", \"sharded\"]"));
    assert!(source.contains("spec[\"desired_shards\"] = desired_shards"));
    assert!(source.contains("--execution-mode distributed,sharded"));
    assert!(source.contains("--execution-scope cluster"));
    assert!(source.contains("--execution-desired-shards \"$NODE_COUNT\""));
    assert!(source.contains("registered node IDs:"));

    let multi_robot_source = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/scripts/run_multi_robot_webots.sh"
    ))
    .expect("scripts/run_multi_robot_webots.sh must be present");
    assert!(multi_robot_source.contains("--nodes <n>"));
    assert!(multi_robot_source.contains("PASS_THROUGH_ARGS+=(--nodes \"$CLUSTER_NODE_COUNT\")"));
    assert!(multi_robot_source.contains("export NM_CLUSTER_NODES=\"$CLUSTER_NODE_COUNT\""));
}

#[test]
fn simulator_launcher_forwards_node_count_to_the_distributed_webots_backend() {
    let source = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/run_sim.sh"))
        .expect("scripts/run_sim.sh must be present");
    assert!(source.contains("--node <n>"));
    assert!(source.contains("--node|--nodes"));
    assert!(source.contains("WEBOTS_PASSTHROUGH_ARGS+=(--nodes \"$CLUSTER_NODE_COUNT\")"));
    assert!(source.contains("start_distributed_tcp_servers"));
    assert!(source.contains("grep -F 'registered node IDs:'"));
    assert!(source.contains("# Keep the orchestrator dashboard visible for simulator runs."));
    assert!(source.contains("--runtime cluster --no-webots --no-diag"));
    assert!(source.contains("target/release/tcp_aer_ipc_bridge"));
    assert!(
        source.contains(
            "cargo build --release --locked --no-default-features --features parallel --bin tcp_aer_ipc_bridge"
        )
    );
    assert!(!source.contains("tcp_aer_ipc_bridge.py"));
    assert!(source.contains("NM_IPC_SOCKET_DIR=$CLUSTER_SOCKET_DIR"));
    assert!(source.contains("--no-webots --no-diag --no-orchestrator-ui"));
    assert!(source.contains("wait_for_distributed_workers_ready"));
    assert!(source.contains("worker processes: ${expected} ("));
    assert!(source.contains("NM_DISTRIBUTED_AUTOSTART=0"));
    assert!(source.contains("--ready-file \"$ready_file\""));
    assert!(source.contains("--arm-file \"$ARM_FILE\""));
    assert!(source.contains("wait_for_environment_bridges_ready"));
    assert!(source.contains("arm_distributed_networks"));
    assert!(source.contains("--cluster-control-action start"));
    assert!(source.contains("wait_for_distributed_networks_armed"));
    assert!(source.contains("prepare_minecraft_token"));
    assert!(source.contains("any launcher-started Java client will inherit it"));
    assert!(source.contains("Unreal launch disabled by --no-engine"));

    let distributed_launcher =
        fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/run_webot.sh"))
            .expect("distributed launcher must be present");
    assert!(distributed_launcher.contains("NM_DISTRIBUTED_AUTOSTART=$DISTRIBUTED_AUTOSTART"));

    let unreal_source = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/scripts/run_unreal_sim.sh"
    ))
    .expect("scripts/run_unreal_sim.sh must be present");
    assert!(unreal_source.contains("--node <count>"));
    assert!(unreal_source.contains("distributed cluster runtime"));

    let unity_source = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/scripts/run_unity_sim.sh"
    ))
    .expect("scripts/run_unity_sim.sh must be present");
    assert!(unity_source.contains("--node <count>"));
    assert!(unity_source.contains("distributed cluster runtime"));
}

#[test]
fn distributed_tcp_bridge_preserves_bounded_protocol_layers() {
    let source = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/tcp_aer_ipc_bridge.rs"
    ))
    .expect("Rust distributed TCP bridge must be present");
    assert!(source.contains("MAX_FRAME_BYTES"));
    assert!(source.contains("MAX_DATAGRAM_BYTES"));
    assert!(source.contains("AER_MAGIC"));
    assert!(source.contains("aer_timestamp(&payload)"));
    assert!(source.contains("biological time"));
    assert!(source.contains("mark_ready"));
    assert!(source.contains("ready_file"));
}

#[test]
fn robot_combo_launchers_and_container_workers_select_cluster_sharding() {
    for path in [
        "scripts/run_celegans_combo_webots.py",
        "scripts/run_drosophila_combo_webots.py",
        "scripts/run_nao_combo_webots.py",
    ] {
        let source = fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path))
            .expect("combo launcher must be present");
        assert!(source.contains("\"distributed,sharded\""));
        assert!(source.contains("\"cluster\""));
        assert!(source.contains("\"NM_DISTRIBUTE_STARTUP_SNAPSHOT\": \"1\""));
        assert!(source.contains("\"NM_DISTRIBUTED_AUTOSTART\": \"1\""));
        assert!(source.contains("hierarchical_sub_shards"));
        assert!(source.contains("hierarchical_nodes"));
        assert!(source.contains("hierarchical_backup_areas"));
        assert!(source.contains("hierarchical_backup_sub_shards"));
        assert!(source.contains("backup_hosts_by_layer"));
        assert!(source.contains("backup_ready"));
    }

    let entrypoint = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/scripts/container_entrypoint.sh"
    ))
    .expect("container entrypoint must be present");
    assert!(entrypoint.contains("--execution-mode distributed,sharded"));
    assert!(entrypoint.contains("--execution-scope cluster"));
    assert!(entrypoint.contains("AARNN_EXECUTION_DESIRED_SHARDS"));
}
