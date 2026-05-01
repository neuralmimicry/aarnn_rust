use aarnn_rust::runtime::{RuntimeConfig, RuntimeManager};
use aarnn_rust::runtime_api::{WorkspaceControlAction, WorkspaceCreateRequest};
use std::path::PathBuf;
use tokio::time::Duration;

fn temp_runtime_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aarnn-runtime-test-{:08x}", fastrand::u32(..)));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn runtime_manager_persists_and_resumes_workspace_state() {
    let root = temp_runtime_dir();
    let runtime = RuntimeManager::new(RuntimeConfig {
        root_dir: root.clone(),
        tick_interval_ms: 5,
        local_worker_limit: 1,
        resume_existing_workspaces: true,
        autosave_steps: 1,
        continuum: None,
        reconcile_interval_ms: 5,
        autoscaler_interval_ms: 50,
        orchestrator_addr: None,
    })
    .await
    .unwrap();

    let detail = runtime
        .create_workspace(
            "alice",
            WorkspaceCreateRequest {
                workspace_id: Some("alpha".to_string()),
                name: Some("Alpha".to_string()),
                ..WorkspaceCreateRequest::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(detail.summary.workspace_id, "alpha");

    runtime
        .control_workspace("alice", "alpha", WorkspaceControlAction::Start)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;

    let stepped = runtime.workspace_detail("alice", "alpha").await.unwrap();
    assert!(stepped.status.step > 0);
    runtime.shutdown().await;
    drop(runtime);

    let resumed = RuntimeManager::new(RuntimeConfig {
        root_dir: root.clone(),
        tick_interval_ms: 5,
        local_worker_limit: 1,
        resume_existing_workspaces: true,
        autosave_steps: 1,
        continuum: None,
        reconcile_interval_ms: 5,
        autoscaler_interval_ms: 50,
        orchestrator_addr: None,
    })
    .await
    .unwrap();
    let detail = resumed.workspace_detail("alice", "alpha").await.unwrap();
    assert!(detail.status.step > 0);
    resumed.shutdown().await;
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn runtime_manager_isolates_users_by_workspace_root() {
    let root = temp_runtime_dir();
    let runtime = RuntimeManager::new(RuntimeConfig {
        root_dir: root.clone(),
        tick_interval_ms: 10,
        local_worker_limit: 1,
        resume_existing_workspaces: true,
        autosave_steps: 10,
        continuum: None,
        reconcile_interval_ms: 10,
        autoscaler_interval_ms: 50,
        orchestrator_addr: None,
    })
    .await
    .unwrap();

    runtime
        .create_workspace(
            "alice",
            WorkspaceCreateRequest {
                workspace_id: Some("shared".to_string()),
                ..WorkspaceCreateRequest::default()
            },
        )
        .await
        .unwrap();
    runtime
        .create_workspace(
            "bob",
            WorkspaceCreateRequest {
                workspace_id: Some("shared".to_string()),
                ..WorkspaceCreateRequest::default()
            },
        )
        .await
        .unwrap();

    let alice = runtime.list_workspaces("alice").await.unwrap();
    let bob = runtime.list_workspaces("bob").await.unwrap();
    assert_eq!(alice.len(), 1);
    assert_eq!(bob.len(), 1);
    assert_eq!(alice[0].workspace_id, "shared");
    assert_eq!(bob[0].workspace_id, "shared");

    let alice_status = runtime.runtime_status("alice").await.unwrap();
    assert_eq!(alice_status.total_users, 1);
    assert_eq!(alice_status.total_workspaces, 1);
    assert_eq!(alice_status.running_workspaces, 0);

    let bob_status = runtime.runtime_status("bob").await.unwrap();
    assert_eq!(bob_status.total_users, 1);
    assert_eq!(bob_status.total_workspaces, 1);
    assert_eq!(bob_status.running_workspaces, 0);

    runtime.shutdown().await;
    let _ = std::fs::remove_dir_all(root);
}
