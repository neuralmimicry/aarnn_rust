use aarnn_rust::runtime::{RuntimeConfig, RuntimeManager};
use aarnn_rust::runtime_api::{
    WorkspaceControlAction, WorkspaceCreateRequest, WorkspaceDetailResponse,
};
use std::path::PathBuf;
use tokio::time::{Duration, Instant};

fn temp_runtime_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aarnn-runtime-test-{:08x}", fastrand::u32(..)));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

async fn wait_for_workspace_step(
    runtime: &RuntimeManager,
    user_id: &str,
    workspace_id: &str,
    min_step: u64,
) -> WorkspaceDetailResponse {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let detail = runtime
            .workspace_detail(user_id, workspace_id)
            .await
            .unwrap();
        if detail.status.step > min_step {
            return detail;
        }
        if Instant::now() >= deadline {
            panic!(
                "workspace '{workspace_id}' did not advance beyond step {min_step}; current step {}",
                detail.status.step
            );
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn runtime_manager_persists_and_resumes_workspace_state() {
    let root = temp_runtime_dir();
    let runtime = RuntimeManager::new(RuntimeConfig {
        root_dir: root.clone(),
        tick_interval_ms: 5,
        local_worker_limit: 1,
        max_loaded_workspaces: usize::MAX,
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
    let stepped = wait_for_workspace_step(&runtime, "alice", "alpha", 0).await;
    assert!(stepped.status.step > 0);
    runtime
        .control_workspace("alice", "alpha", WorkspaceControlAction::Save)
        .await
        .unwrap();
    runtime.shutdown().await;
    drop(runtime);

    let resumed = RuntimeManager::new(RuntimeConfig {
        root_dir: root.clone(),
        tick_interval_ms: 5,
        local_worker_limit: 1,
        max_loaded_workspaces: usize::MAX,
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
        max_loaded_workspaces: usize::MAX,
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

#[tokio::test]
async fn runtime_manager_lists_requested_workspace_owners_in_order() {
    let root = temp_runtime_dir();
    let runtime = RuntimeManager::new(RuntimeConfig {
        root_dir: root.clone(),
        tick_interval_ms: 10,
        local_worker_limit: 1,
        max_loaded_workspaces: usize::MAX,
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
                workspace_id: Some("alpha".to_string()),
                name: Some("Alice Alpha".to_string()),
                ..WorkspaceCreateRequest::default()
            },
        )
        .await
        .unwrap();
    runtime
        .create_workspace(
            "system",
            WorkspaceCreateRequest {
                workspace_id: Some("alpha".to_string()),
                name: Some("System Alpha".to_string()),
                ..WorkspaceCreateRequest::default()
            },
        )
        .await
        .unwrap();

    let combined = runtime
        .list_workspaces_for_users(["alice", "system"])
        .await
        .unwrap();
    assert_eq!(combined.len(), 2);
    assert_eq!(combined[0].owner_id, "alice");
    assert_eq!(combined[0].workspace_id, "alpha");
    assert_eq!(combined[1].owner_id, "system");
    assert_eq!(combined[1].workspace_id, "alpha");

    runtime.shutdown().await;
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn runtime_manager_enforces_loaded_workspace_cap_on_create_and_restart() {
    let root = temp_runtime_dir();
    let initial = RuntimeManager::new(RuntimeConfig {
        root_dir: root.clone(),
        max_loaded_workspaces: 2,
        continuum: None,
        ..RuntimeConfig::default()
    })
    .await
    .unwrap();
    for workspace_id in ["alpha", "beta"] {
        initial
            .create_workspace(
                "alice",
                WorkspaceCreateRequest {
                    workspace_id: Some(workspace_id.to_string()),
                    ..WorkspaceCreateRequest::default()
                },
            )
            .await
            .unwrap();
    }
    let error = initial
        .create_workspace(
            "alice",
            WorkspaceCreateRequest {
                workspace_id: Some("gamma".to_string()),
                ..WorkspaceCreateRequest::default()
            },
        )
        .await
        .expect_err("workspace creation must stop at the resident-engine cap");
    assert!(error.to_string().contains("memory limit reached"));
    initial.shutdown().await;
    drop(initial);

    let restarted = RuntimeManager::new(RuntimeConfig {
        root_dir: root.clone(),
        max_loaded_workspaces: 1,
        continuum: None,
        ..RuntimeConfig::default()
    })
    .await
    .unwrap();
    assert_eq!(restarted.list_workspaces("alice").await.unwrap().len(), 1);
    restarted.shutdown().await;
    let _ = std::fs::remove_dir_all(root);
}
