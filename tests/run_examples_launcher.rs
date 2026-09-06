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
        source.contains("--features \"engine_runtime,ui\""),
        "the native example must opt into only its documented local UI profile"
    );
    assert!(
        source.contains("--advertise-addr 127.0.0.1:$NODE1_PORT")
            && source.contains("--advertise-addr 127.0.0.1:$NODE2_PORT"),
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
fn webcluster_launcher_uses_the_same_local_profile_and_dashboard_output() {
    let source = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/run_webcluster.sh"))
        .expect("run_webcluster.sh must be present");
    assert!(
        source.contains("cargo build --release --locked --no-default-features \\\n        --bin aarnn_rust --bin web_ui"),
        "the webcluster launcher must not compile the authenticated all-features profile"
    );
    assert!(
        source.contains("--features \"engine_runtime,ui\""),
        "the webcluster launcher must use the documented local native UI profile"
    );
    assert!(
        source.contains("--advertise-addr 127.0.0.1:$NODE1_PORT")
            && source.contains("--advertise-addr 127.0.0.1:$NODE2_PORT"),
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
