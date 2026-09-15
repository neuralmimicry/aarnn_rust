use std::fs;

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
    assert!(source.contains("tcp_aer_ipc_bridge.py"));
    assert!(source.contains("NM_IPC_SOCKET_DIR=$CLUSTER_SOCKET_DIR"));
    assert!(source.contains("--no-webots --no-diag --no-orchestrator-ui"));

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
        "/scripts/tcp_aer_ipc_bridge.py"
    ))
    .expect("distributed TCP bridge must be present");
    assert!(source.contains("MAX_FRAME_BYTES"));
    assert!(source.contains("MAX_DATAGRAM_BYTES"));
    assert!(source.contains("AER_MAGIC"));
    assert!(source.contains("aer_timestamp(payload)"));
    assert!(source.contains("never use wall-clock arrival time"));
}
