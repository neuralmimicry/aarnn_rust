use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn launcher() -> String {
    fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/run_examples.sh"))
        .expect("run_examples.sh must be present")
}

#[test]
fn example_launcher_uses_the_complete_feature_profile() {
    let source = launcher();
    assert!(
        source.contains("SCRIPT_DIR=\"$(cd -- \"$(dirname -- \"${BASH_SOURCE[0]}\")\" && pwd)\""),
        "the launcher must anchor relative paths to its own checkout"
    );
    assert!(
        source.contains("cargo build --release --locked --all-features \\\n        --bin aarnn_rust --bin web_ui"),
        "the example launcher must build both binaries with the complete feature graph"
    );
    assert!(
        source.contains("--advertise-addr \"127.0.0.1:$NODE1_PORT\"")
            && source.contains("--advertise-addr \"127.0.0.1:$NODE2_PORT\""),
        "local nodes must advertise reachable loopback endpoints rather than wildcard bind addresses"
    );
    assert!(source.contains("scripts/local_management_env.py"));
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
        source.contains("cargo build --release --locked --all-features \\\n        --bin aarnn_rust --bin web_ui"),
        "the webcluster launcher must compile the complete feature graph"
    );
    assert!(
        source.contains("--advertise-addr \"127.0.0.1:$NODE1_PORT\"")
            && source.contains("--advertise-addr \"127.0.0.1:$NODE2_PORT\""),
        "local nodes must advertise reachable loopback endpoints rather than wildcard bind addresses"
    );
    assert!(
        source.contains("$WEB_UI_URL/api/config"),
        "the webcluster launcher must verify the dashboard before reporting its URL"
    );
    assert!(
        source.contains("echo \"Web dashboard URL (port $WEB_UI_PORT): $WEB_UI_URL\""),
        "the webcluster launcher must print the exact dashboard URL and port"
    );
    assert!(source.contains("scripts/local_management_env.py"));
}

#[test]
fn management_enabled_launchers_use_the_shared_profile_gate() {
    for path in [
        "scripts/run_cluster.sh",
        "scripts/run_sim.sh",
        "scripts/run_multi_robot_webots.sh",
        "run_webot.sh",
        "run_webcluster.sh",
    ] {
        let source = fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path))
            .expect("local launcher must be present");
        assert!(
            source.contains("local_management_env.py")
                || source.contains("webots_prepare_management_env"),
            "{path} must prepare management credentials through the selected feature profile"
        );
    }
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
    assert!(source.contains("local_management_env.py"));
}

#[test]
fn webots_launcher_uses_explicit_profile_with_complete_graph_opt_in() {
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
    assert!(source.contains("webots_runtime_profile.sh"));
    assert!(source.contains("webots_prepare_management_env"));
    assert!(source.contains("--no-default-features"));
    assert!(source.contains("--all-features"));
    assert!(source.contains("SUPERVISED_PIDS"));
    assert!(source.contains("wait_for_runtime_or_webots"));
    assert!(source.contains("Stopping Webots because its neural runtime is no longer available."));
    assert!(source.contains("spec[\"execution_modes\"] = [\"distributed\", \"sharded\"]"));
    assert!(source.contains("spec[\"desired_shards\"] = desired_shards"));
    assert!(source.contains("--execution-mode distributed,sharded"));
    assert!(source.contains("--execution-scope cluster"));
    assert!(source.contains("--execution-desired-shards \"$NODE_COUNT\""));
    assert!(source.contains("registered node IDs:"));
    assert!(source.contains("node_cmd+=(--headless-ipc)"));
    assert!(source.contains("elif [ \"$NODE_UI\" -eq 1 ]; then\n            node_cmd+=(--ui)"));

    let multi_robot_source = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/scripts/run_multi_robot_webots.sh"
    ))
    .expect("scripts/run_multi_robot_webots.sh must be present");
    assert!(multi_robot_source.contains("--nodes <n>"));
    assert!(multi_robot_source.contains("PASS_THROUGH_ARGS+=(--nodes \"$CLUSTER_NODE_COUNT\")"));
    assert!(multi_robot_source.contains("export NM_CLUSTER_NODES=\"$CLUSTER_NODE_COUNT\""));
    assert!(multi_robot_source.contains("--all-features"));
    assert!(multi_robot_source.contains("webots_prepare_management_env"));
}

#[test]
fn webots_profile_args_survive_single_line_shell_parsing() {
    let profile =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/webots_runtime_profile.sh");
    let output = Command::new("bash")
        .args([
            "-c",
            "source \"$1\"; webots_cargo_profile_args engine_runtime,ui,robot_io,cuda",
            "bash",
            profile.to_str().expect("profile path must be UTF-8"),
        ])
        .output()
        .expect("bash must execute the Webots profile helper");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout)
            .expect("profile output must be UTF-8")
            .trim(),
        "--no-default-features --features engine_runtime,ui,robot_io,cuda"
    );

    let output = Command::new("bash")
        .args([
            "-c",
            "source \"$1\"; webots_cargo_profile_args all-features",
            "bash",
            profile.to_str().expect("profile path must be UTF-8"),
        ])
        .output()
        .expect("bash must execute the Webots profile helper");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout)
            .expect("profile output must be UTF-8")
            .trim(),
        "--all-features"
    );
}

#[test]
fn simulator_launcher_forwards_node_count_to_the_distributed_webots_backend() {
    let source = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/run_sim.sh"))
        .expect("scripts/run_sim.sh must be present");
    assert!(source.contains("--node <n>"));
    assert!(source.contains("--node|--nodes"));
    assert!(source.contains("WEBOTS_PASSTHROUGH_ARGS+=(--nodes \"$CLUSTER_NODE_COUNT\")"));
    assert!(source.contains("WEBOTS_PASSTHROUGH_ARGS+=(--no-build)"));
    assert!(source.contains("start_distributed_tcp_servers"));
    assert!(source.contains("grep -F 'registered node IDs:'"));
    assert!(source.contains("# Keep the orchestrator dashboard visible for simulator runs."));
    assert!(source.contains("--runtime cluster --no-webots --no-diag"));
    assert!(source.contains("target/release/tcp_aer_ipc_bridge"));
    assert!(
        source.contains("cargo build --release --locked --all-features --bin tcp_aer_ipc_bridge")
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
fn gated_simulator_frontends_arm_after_their_environment_handshake() {
    let source = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/run_sim.sh"))
        .expect("scripts/run_sim.sh must be present");
    assert!(source.contains("source \"$ROOT_DIR/scripts/webots_runtime_profile.sh\""));
    assert!(source.contains("-u NM_GRPC_TLS_CERT"));
    assert!(source.contains("webots_profile_has_feature \"$runtime_features\" management_v1"));

    let minecraft = source
        .find("minecraft_bridge_pid=\"$!\"")
        .expect("Minecraft bridge startup must be present");
    let minecraft_start = &source[minecraft..];
    let minecraft_wait = minecraft_start
        .find("wait-bridge --pid \"$minecraft_bridge_pid\"")
        .expect("Minecraft must wait for the companion bridge");
    let minecraft_arm = minecraft_start
        .find("arm_distributed_networks")
        .expect("Minecraft distributed mode must arm after its bridge is ready");
    let minecraft_launch = minecraft_start
        .find("if [ \"$minecraft_edition\" = bedrock ]")
        .expect("Minecraft engine launch must follow bridge setup");
    assert!(minecraft_wait < minecraft_arm && minecraft_arm < minecraft_launch);

    let unity = source
        .find("  unity)\n")
        .expect("Unity dispatch must be present");
    let unity_start = &source[unity..];
    let unity_wait = unity_start
        .find("wait_for_environment_bridges_ready")
        .expect("Unity distributed mode must wait for its bridge handshake");
    let unity_arm = unity_start
        .find("arm_distributed_networks")
        .expect("Unity distributed mode must arm after its bridge handshake");
    let unity_serve = unity_start
        .find("serve_and_wait")
        .expect("Unity must retain its editor-driven wait path");
    assert!(unity_wait < unity_arm && unity_arm < unity_serve);

    let unreal = source
        .find("  unreal)\n")
        .expect("Unreal dispatch must be present");
    let unreal_start = &source[unreal..];
    let unreal_wait = unreal_start
        .find("wait_for_environment_bridges_ready")
        .expect("Unreal distributed mode must wait for its bridge handshake");
    let unreal_arm = unreal_start
        .find("arm_distributed_networks")
        .expect("Unreal distributed mode must arm after its bridge handshake");
    assert!(unreal_wait < unreal_arm);
}

#[test]
fn webgl_launcher_uses_the_same_explicit_local_runtime_profile() {
    let source = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/run_sim.sh"))
        .expect("scripts/run_sim.sh must be present");
    let launch = source
        .split("launch_webgl() {")
        .nth(1)
        .expect("WebGL launcher must be present");
    assert!(launch.contains("WEBGL_RUNTIME_FEATURES"));
    assert!(launch.contains("webots_cargo_profile_args \"$WEBGL_RUNTIME_FEATURES\""));
    assert!(
        launch.contains(
            "cargo build --release --locked \"${webgl_runtime_args[@]}\" --bin aarnn_rust"
        )
    );
    assert!(launch.contains(
        "cargo build --release --locked --no-default-features --features engine_runtime --bin web_ui"
    ));
    assert!(launch.contains("export NM_WEBOTS_RUNTIME_FEATURES=\"$WEBGL_RUNTIME_FEATURES\""));
    assert!(launch.contains("web_ui_env=("));
    assert!(launch.contains("-u NM_GRPC_TLS_CERT"));
    assert!(launch.contains("--no-webots --no-diag --no-orchestrator-ui"));
    assert!(launch.contains("--node-ui-hidden"));
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
        assert!(source.contains("webots_runtime_profile"));
        assert!(source.contains("prepare_management_environment"));
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
