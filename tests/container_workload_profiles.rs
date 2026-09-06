use std::path::Path;
use std::process::Command;

fn workload_script() -> &'static Path {
    Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/scripts/container_workloads.sh"
    ))
}

#[test]
fn container_entrypoint_forwards_provider_bound_node_identity() {
    let entrypoint = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/scripts/container_entrypoint.sh"
    ))
    .expect("container entrypoint must be present");
    assert!(entrypoint.contains("stable-node"));
    assert!(entrypoint.contains("default_args+=(--node-id \"${AARNN_NODE_ID}\")"));
    assert!(entrypoint.contains("A deployment may provide a host- or provider-bound identity"));
}

#[test]
fn stable_container_workloads_use_the_authenticated_live_profile() {
    let script = workload_script().to_str().expect("UTF-8 repository path");
    let output = Command::new("bash")
        .arg("-c")
        .arg(format!(
            "source \"$1\"; aarnn_container_validate_workload stable-orchestrator; aarnn_container_validate_workload stable-node; printf '%s\\n' \"$(aarnn_container_workload_features stable-orchestrator)\" \"$(aarnn_container_workload_features stable-node)\"",
        ))
        .arg("aarnn-container-workload-test")
        .arg(script)
        .output()
        .expect("run workload metadata helper");

    assert!(output.status.success(), "helper failed: {:?}", output);
    let stdout = String::from_utf8(output.stdout).expect("helper output is UTF-8");
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        vec!["stable_runtime_workload", "stable_runtime_workload"]
    );
}

#[test]
fn stable_container_workloads_are_known_to_the_build_metadata() {
    let script = workload_script().to_str().expect("UTF-8 repository path");
    let output = Command::new("bash")
        .arg("-c")
        .arg("source \"$1\"; aarnn_container_workload_names")
        .arg("aarnn-container-workload-test")
        .arg(script)
        .output()
        .expect("run workload metadata helper");

    assert!(output.status.success(), "helper failed: {:?}", output);
    let stdout = String::from_utf8(output.stdout).expect("helper output is UTF-8");
    assert!(stdout.lines().any(|line| line == "stable-orchestrator"));
    assert!(stdout.lines().any(|line| line == "stable-node"));
}
