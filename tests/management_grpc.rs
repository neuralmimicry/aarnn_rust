use aarnn_rust::deterministic::{BrainId, LeaseTerm, LogicalTag, ShardId};
use aarnn_rust::generated_management::proto::management_client::ManagementClient;
use aarnn_rust::generated_management::proto::management_server::ManagementServer;
use aarnn_rust::generated_management::proto::{
    OperationKind, OperationRequest, RequestContext, StatusRequest,
};
use aarnn_rust::management::{
    AuthenticatedPrincipal, Capability, ManagementGrpcService, ManagementOperationDispatcher,
    ManagementOrchestrator, MutationContext, PersistedManagementOrchestrator,
    PlacementActivationDispatcher, Policy, SecuredManagementGrpcService,
    management_auth_interceptor,
};
use aarnn_rust::placement::{
    PlacementCommand, PlacementCommandKind, PlacementConstraints, PlacementRequest,
    ResourceObservation, ShardDemand,
};
use tokio::sync::oneshot;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::Request;

fn service() -> ManagementGrpcService {
    let mut policy = Policy::default();
    policy.grant("operator", Capability::Operate);
    policy.grant("operator", Capability::Reset);
    policy.grant("operator", Capability::Export);
    ManagementGrpcService::new(ManagementOrchestrator::new(LeaseTerm::INITIAL, policy))
}

fn context(id: &str, key: &str, term: u64, version: u64) -> RequestContext {
    RequestContext {
        request_id: id.to_owned(),
        idempotency_key: key.to_owned(),
        expected_resource_version: version,
        observed_leader_term: term,
    }
}

fn placement_command_json() -> String {
    let command = PlacementCommand::new(
        "placement-request-1",
        "placement-idempotency-1",
        "operator",
        BrainId::new(42).unwrap(),
        0,
        LeaseTerm::INITIAL,
        PlacementCommandKind::PlanPlacement(PlacementRequest {
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
                node_id: "worker-a".to_owned(),
                device_id: "worker-a-cpu".to_owned(),
                healthy: true,
                enrolled: true,
                compute_authorised: true,
                failure_domain: "rack-a".to_owned(),
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
            intent: aarnn_rust::placement::PlacementIntent::Automatic,
        }),
    )
    .unwrap();
    serde_json::to_string(&command).unwrap()
}

fn apply_placement_command_json() -> String {
    let proposal: PlacementCommand = serde_json::from_str(&placement_command_json()).unwrap();
    let PlacementCommandKind::PlanPlacement(request) = proposal.kind else {
        panic!("fixture must be a planning command");
    };
    let plan = aarnn_rust::placement::PlacementPlanner
        .plan(request)
        .unwrap();
    serde_json::to_string(
        &PlacementCommand::new(
            "placement-apply-1",
            "placement-apply-key-1",
            "operator",
            BrainId::new(42).unwrap(),
            0,
            LeaseTerm::INITIAL,
            PlacementCommandKind::ApplyPlacement(plan),
        )
        .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn generated_management_client_reaches_server_and_preserves_policy_contract() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("management listener");
    let address = listener.local_addr().expect("management address");
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(ManagementServer::new(service()))
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async move {
                let _ = shutdown_rx.await;
            })
            .await
            .expect("management server");
    });

    let mut client = ManagementClient::connect(format!("http://{address}"))
        .await
        .expect("generated management client connection");
    let status = client
        .get_status(StatusRequest {
            brain_id: "brain-a".to_owned(),
        })
        .await
        .expect("status")
        .into_inner();
    assert_eq!(status.schema_version, 2);
    assert_eq!(status.leader_term, 1);

    let request = OperationRequest {
        principal_id: "operator".to_owned(),
        brain_id: "brain-a".to_owned(),
        kind: OperationKind::Start as i32,
        context: Some(context("request-1", "key-1", 1, 0)),
    };
    let first = client
        .submit_operation(Request::new(request.clone()))
        .await
        .expect("operation accepted")
        .into_inner();
    let duplicate = client
        .submit_operation(Request::new(request))
        .await
        .expect("idempotent retry")
        .into_inner();
    assert_eq!(first.operation_id, duplicate.operation_id);
    assert_eq!(first.state, "pending");
    assert_eq!(first.resource_version, 1);

    let lookup = client
        .get_operation(aarnn_rust::generated_management::proto::OperationLookup {
            operation_id: first.operation_id,
            brain_id: "brain-a".to_owned(),
            observed_leader_term: 1,
        })
        .await
        .expect("operation lookup")
        .into_inner();
    assert_eq!(lookup.operation_id, first.operation_id);
    assert_eq!(lookup.state, "pending");

    let denied = client
        .submit_operation(Request::new(OperationRequest {
            principal_id: "observer".to_owned(),
            brain_id: "brain-a".to_owned(),
            kind: OperationKind::Start as i32,
            context: Some(context("request-2", "key-2", 1, 1)),
        }))
        .await
        .expect_err("unprivileged principal must be denied");
    assert_eq!(denied.code(), tonic::Code::PermissionDenied);

    let stale = client
        .submit_operation(Request::new(OperationRequest {
            principal_id: "operator".to_owned(),
            brain_id: "brain-a".to_owned(),
            kind: OperationKind::Stop as i32,
            context: Some(context("request-3", "key-3", 2, 1)),
        }))
        .await
        .expect_err("stale term must be rejected");
    assert_eq!(stale.code(), tonic::Code::FailedPrecondition);

    let _ = shutdown_tx.send(());
    server.await.expect("management server join");
}

#[test]
fn management_endpoint_auth_fails_closed_without_a_configured_token() {
    let result = management_auth_interceptor(Request::new(()));
    assert_eq!(
        result.expect_err("missing token must fail").code(),
        tonic::Code::Unauthenticated
    );
}

#[tokio::test]
async fn management_plan_placement_returns_a_verified_proposal() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("management listener");
    let address = listener.local_addr().expect("management address");
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(ManagementServer::new(service()))
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async move {
                let _ = shutdown_rx.await;
            })
            .await
            .expect("management server");
    });

    let mut client = ManagementClient::connect(format!("http://{address}"))
        .await
        .expect("generated management client connection");
    let response = client
        .plan_placement(
            aarnn_rust::generated_management::proto::PlacementCommandRequest {
                command_json: placement_command_json(),
                cutover_json: String::new(),
                repartition_json: String::new(),
                stable_worker_activation_json: String::new(),
            },
        )
        .await
        .expect("placement proposal accepted")
        .into_inner();
    assert_eq!(response.schema_version, 2);
    assert!(!response.command_digest.is_empty());
    let plan: aarnn_rust::placement::PlacementPlan =
        serde_json::from_str(&response.plan_json).expect("plan JSON");
    plan.verify().expect("management returns a verified plan");
    assert_eq!(plan.brain_id, BrainId::new(42).unwrap());

    let _ = shutdown_tx.send(());
    server.await.expect("management server join");
}

#[tokio::test]
async fn management_apply_placement_publishes_a_fenced_registry_receipt() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("management listener");
    let address = listener.local_addr().expect("management address");
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(ManagementServer::new(service()))
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async move {
                let _ = shutdown_rx.await;
            })
            .await
            .expect("management server");
    });

    let mut client = ManagementClient::connect(format!("http://{address}"))
        .await
        .expect("generated management client connection");
    let response = client
        .apply_placement(
            aarnn_rust::generated_management::proto::PlacementCommandRequest {
                command_json: apply_placement_command_json(),
                cutover_json: String::new(),
                repartition_json: String::new(),
                stable_worker_activation_json: String::new(),
            },
        )
        .await
        .expect("placement apply accepted")
        .into_inner();
    let receipt: aarnn_rust::placement_registry::PlacementApplyReceipt =
        serde_json::from_str(&response.receipt_json).expect("receipt JSON");
    assert_eq!(receipt.resource_version, 1);
    let registry: aarnn_rust::placement_registry::PlacementRegistry =
        serde_json::from_str(&response.registry_json).expect("registry JSON");
    assert_eq!(registry.authorities.len(), 1);
    assert_eq!(registry.resource_version, 1);

    // The first apply establishes the controller's committed residence
    // boundary. An otherwise valid automatic move is rejected until the
    // configured residence interval has elapsed.
    let initial: PlacementCommand = serde_json::from_str(&placement_command_json()).unwrap();
    let PlacementCommandKind::PlanPlacement(mut request) = initial.kind else {
        panic!("fixture must be a planning command");
    };
    request.effective_tag = LogicalTag::new(10, 0);
    request.demands[0].preferred_node = Some("worker-b".to_owned());
    let mut worker_b = request.resources[0].clone();
    worker_b.node_id = "worker-b".to_owned();
    worker_b.device_id = "worker-b-cpu".to_owned();
    worker_b.failure_domain = "rack-b".to_owned();
    worker_b.cpu_pressure_milli = 10;
    request.resources.push(worker_b);
    let move_command = PlacementCommand::new(
        "placement-plan-move",
        "placement-plan-move-key",
        "operator",
        BrainId::new(42).unwrap(),
        1,
        LeaseTerm::INITIAL,
        PlacementCommandKind::PlanPlacement(request),
    )
    .unwrap();
    let blocked = client
        .plan_placement(
            aarnn_rust::generated_management::proto::PlacementCommandRequest {
                command_json: serde_json::to_string(&move_command).unwrap(),
                cutover_json: String::new(),
                repartition_json: String::new(),
                stable_worker_activation_json: String::new(),
            },
        )
        .await
        .expect_err("automatic movement must respect residence hysteresis");
    assert_eq!(blocked.code(), tonic::Code::FailedPrecondition);
    assert!(blocked.message().contains("residence"));

    let _ = shutdown_tx.send(());
    server.await.expect("management server join");
}

#[tokio::test]
async fn management_migration_rpc_journals_and_advances_fenced_operation() {
    use aarnn_rust::migration_operation::{
        MigrationKind, MigrationOperation, MigrationPhase, MigrationProgress, MigrationRequest,
        MigrationTransition,
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("management listener");
    let address = listener.local_addr().expect("management address");
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(ManagementServer::new(service()))
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async move {
                let _ = shutdown_rx.await;
            })
            .await
            .expect("management server");
    });
    let mut client = ManagementClient::connect(format!("http://{address}"))
        .await
        .expect("generated management client connection");
    let request = MigrationRequest {
        request_id: "migration-rpc-request".to_owned(),
        idempotency_key: "migration-rpc-key".to_owned(),
        brain_id: BrainId::new(42).unwrap(),
        observed_leader_term: LeaseTerm::INITIAL,
        expected_resource_version: 0,
        kind: MigrationKind::Consolidate,
        source_plan_digest: aarnn_rust::deterministic::StateDigest([1; 16]),
        target_plan_digest: aarnn_rust::deterministic::StateDigest([2; 16]),
        total_shards: 1,
        total_bytes: 10,
    };
    let submitted = client
        .submit_migration(
            aarnn_rust::generated_management::proto::MigrationCommandRequest {
                principal_id: "operator".to_owned(),
                command_json: serde_json::to_string(&request).unwrap(),
                group_json: String::new(),
            },
        )
        .await
        .expect("migration submission")
        .into_inner();
    let operation: MigrationOperation = serde_json::from_str(&submitted.operation_json).unwrap();
    assert_eq!(operation.phase, MigrationPhase::Prepared);
    assert_eq!(operation.operation_id, 1);

    let transition = MigrationTransition {
        operation_id: operation.operation_id,
        observed_leader_term: LeaseTerm::INITIAL,
        expected_resource_version: operation.resource_version,
        next_phase: MigrationPhase::Reserving,
        progress: MigrationProgress::new(1, 10).unwrap(),
        error_code: None,
    };
    let advanced = client
        .advance_migration(
            aarnn_rust::generated_management::proto::MigrationAdvanceRequest {
                principal_id: "operator".to_owned(),
                transition_json: serde_json::to_string(&transition).unwrap(),
                brain_id: 42,
                group_update_json: String::new(),
            },
        )
        .await
        .expect("migration transition")
        .into_inner();
    let operation: MigrationOperation = serde_json::from_str(&advanced.operation_json).unwrap();
    assert_eq!(operation.phase, MigrationPhase::Reserving);

    let looked_up = client
        .get_migration(aarnn_rust::generated_management::proto::MigrationLookup {
            brain_id: 42,
            operation_id: operation.operation_id,
            observed_leader_term: 1,
        })
        .await
        .expect("migration lookup")
        .into_inner();
    let recovered: MigrationOperation = serde_json::from_str(&looked_up.operation_json).unwrap();
    assert_eq!(recovered.phase, MigrationPhase::Reserving);
    assert_eq!(looked_up.schema_version, 2);

    let _ = shutdown_tx.send(());
    server.await.expect("management server join");
}

#[tokio::test]
async fn management_group_barrier_is_journaled_and_status_is_queryable() {
    use aarnn_rust::migration_group::{
        MigrationGroupAction, MigrationGroupSpec, MigrationGroupUpdate,
    };
    use aarnn_rust::migration_operation::{
        MigrationKind, MigrationOperation, MigrationPhase, MigrationProgress, MigrationRequest,
        MigrationTransition,
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("management listener");
    let address = listener.local_addr().expect("management address");
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(ManagementServer::new(service()))
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async move {
                let _ = shutdown_rx.await;
            })
            .await
            .expect("management server");
    });
    let mut client = ManagementClient::connect(format!("http://{address}"))
        .await
        .expect("generated management client connection");
    let request = MigrationRequest {
        request_id: "group-rpc-request".to_owned(),
        idempotency_key: "group-rpc-key".to_owned(),
        brain_id: BrainId::new(42).unwrap(),
        observed_leader_term: LeaseTerm::INITIAL,
        expected_resource_version: 0,
        kind: MigrationKind::MigrateBrain,
        source_plan_digest: aarnn_rust::deterministic::StateDigest([1; 16]),
        target_plan_digest: aarnn_rust::deterministic::StateDigest([2; 16]),
        total_shards: 1,
        total_bytes: 10,
    };
    let spec = MigrationGroupSpec {
        brain_id: BrainId::new(42).unwrap(),
        leader_term: LeaseTerm::INITIAL,
        topology_generation: aarnn_rust::deterministic::TopologyGeneration::INITIAL,
        partition_generation: aarnn_rust::deterministic::PartitionGeneration::INITIAL,
        shard_ids: vec![ShardId::new(11).unwrap()],
    };
    let submitted = client
        .submit_migration(
            aarnn_rust::generated_management::proto::MigrationCommandRequest {
                principal_id: "operator".to_owned(),
                command_json: serde_json::to_string(&request).unwrap(),
                group_json: serde_json::to_string(&spec).unwrap(),
            },
        )
        .await
        .expect("group migration submission")
        .into_inner();
    let operation: MigrationOperation = serde_json::from_str(&submitted.operation_json).unwrap();
    let journal: serde_json::Value = serde_json::from_str(&submitted.journal_json).unwrap();
    assert!(journal["groups"][operation.operation_id.to_string()].is_object());

    let transition = MigrationTransition {
        operation_id: operation.operation_id,
        observed_leader_term: LeaseTerm::INITIAL,
        expected_resource_version: operation.resource_version,
        next_phase: MigrationPhase::Reserving,
        progress: MigrationProgress::new(1, 10).unwrap(),
        error_code: None,
    };
    let update = MigrationGroupUpdate {
        operation_id: operation.operation_id,
        observed_leader_term: LeaseTerm::INITIAL,
        expected_resource_version: operation.resource_version,
        action: MigrationGroupAction::BeginTransfer {
            shard_id: ShardId::new(11).unwrap(),
        },
    };
    let advanced = client
        .advance_migration(
            aarnn_rust::generated_management::proto::MigrationAdvanceRequest {
                principal_id: "operator".to_owned(),
                transition_json: serde_json::to_string(&transition).unwrap(),
                brain_id: 42,
                group_update_json: serde_json::to_string(&update).unwrap(),
            },
        )
        .await
        .expect("group barrier advance")
        .into_inner();
    let advanced_operation: MigrationOperation =
        serde_json::from_str(&advanced.operation_json).unwrap();
    assert_eq!(advanced_operation.phase, MigrationPhase::Reserving);
    assert!(advanced_operation.resource_version > operation.resource_version);

    let status = client
        .get_migration_status(aarnn_rust::generated_management::proto::MigrationLookup {
            brain_id: 42,
            operation_id: operation.operation_id,
            observed_leader_term: 1,
        })
        .await
        .expect("migration status")
        .into_inner();
    let status_operation: MigrationOperation =
        serde_json::from_str(&status.operation_json).unwrap();
    assert_eq!(status_operation.operation_id, operation.operation_id);

    let _ = shutdown_tx.send(());
    server.await.expect("management server join");
}

#[tokio::test]
async fn management_migration_cancel_enforces_fences_and_rejects_committed_work() {
    use aarnn_rust::generated_management::proto::MigrationCancelRequest;
    use aarnn_rust::generated_management::proto::management_server::Management;
    use aarnn_rust::migration_operation::{
        MigrationCancellation, MigrationKind, MigrationOperation, MigrationPhase,
        MigrationProgress, MigrationRequest, MigrationTransition,
    };

    let service = service();
    let request = MigrationRequest {
        request_id: "cancel-rpc-request".to_owned(),
        idempotency_key: "cancel-rpc-key".to_owned(),
        brain_id: BrainId::new(42).unwrap(),
        observed_leader_term: LeaseTerm::INITIAL,
        expected_resource_version: 0,
        kind: MigrationKind::Move,
        source_plan_digest: aarnn_rust::deterministic::StateDigest([3; 16]),
        target_plan_digest: aarnn_rust::deterministic::StateDigest([4; 16]),
        total_shards: 1,
        total_bytes: 10,
    };
    let submitted = service
        .submit_migration(Request::new(
            aarnn_rust::generated_management::proto::MigrationCommandRequest {
                principal_id: "operator".to_owned(),
                command_json: serde_json::to_string(&request).unwrap(),
                group_json: String::new(),
            },
        ))
        .await
        .expect("migration submission")
        .into_inner();
    let operation: MigrationOperation = serde_json::from_str(&submitted.operation_json).unwrap();

    let stale = service
        .cancel_migration(Request::new(MigrationCancelRequest {
            principal_id: "operator".to_owned(),
            brain_id: 42,
            operation_id: operation.operation_id,
            observed_leader_term: 2,
            expected_resource_version: operation.resource_version,
            reason: "stale term".to_owned(),
        }))
        .await
        .expect_err("stale term must reject cancellation")
        .code();
    assert_eq!(stale, tonic::Code::FailedPrecondition);

    let version_conflict = service
        .cancel_migration(Request::new(MigrationCancelRequest {
            principal_id: "operator".to_owned(),
            brain_id: 42,
            operation_id: operation.operation_id,
            observed_leader_term: LeaseTerm::INITIAL.raw(),
            expected_resource_version: 0,
            reason: "stale version".to_owned(),
        }))
        .await
        .expect_err("stale resource version must reject cancellation")
        .code();
    assert_eq!(version_conflict, tonic::Code::FailedPrecondition);

    let cancelled = service
        .cancel_migration(Request::new(MigrationCancelRequest {
            principal_id: "operator".to_owned(),
            brain_id: 42,
            operation_id: operation.operation_id,
            observed_leader_term: LeaseTerm::INITIAL.raw(),
            expected_resource_version: operation.resource_version,
            reason: "operator requested consolidation rollback".to_owned(),
        }))
        .await
        .expect("fenced cancellation")
        .into_inner();
    let cancelled: MigrationOperation = serde_json::from_str(&cancelled.operation_json).unwrap();
    assert_eq!(cancelled.phase, MigrationPhase::Aborted);
    assert!(
        cancelled
            .error_code
            .as_deref()
            .is_some_and(|code| code.starts_with("cancellation_requested:"))
    );

    // A new operation may use the same brain only after the prior operation
    // is terminal. Drive it to a fully evidenced commit, then prove that the
    // cancellation RPC cannot revive its old writer.
    let committed_request = MigrationRequest {
        request_id: "committed-rpc-request".to_owned(),
        idempotency_key: "committed-rpc-key".to_owned(),
        expected_resource_version: cancelled.resource_version,
        ..request
    };
    let submitted = service
        .submit_migration(Request::new(
            aarnn_rust::generated_management::proto::MigrationCommandRequest {
                principal_id: "operator".to_owned(),
                command_json: serde_json::to_string(&committed_request).unwrap(),
                group_json: String::new(),
            },
        ))
        .await
        .expect("second migration submission")
        .into_inner();
    let mut current: MigrationOperation = serde_json::from_str(&submitted.operation_json).unwrap();
    for phase in [
        MigrationPhase::Reserving,
        MigrationPhase::Transferring,
        MigrationPhase::CatchingUp,
        MigrationPhase::Draining,
        MigrationPhase::CutoverReady,
        MigrationPhase::Committed,
    ] {
        let progress = MigrationProgress {
            completed_shards: if phase == MigrationPhase::Committed {
                1
            } else {
                0
            },
            total_shards: 1,
            transferred_bytes: if phase == MigrationPhase::Committed {
                10
            } else {
                0
            },
            total_bytes: 10,
            cut_tag: (phase == MigrationPhase::Committed).then_some(LogicalTag::ZERO),
        };
        let transition = MigrationTransition {
            operation_id: current.operation_id,
            observed_leader_term: LeaseTerm::INITIAL,
            expected_resource_version: current.resource_version,
            next_phase: phase,
            progress,
            error_code: None,
        };
        let response = service
            .advance_migration(Request::new(
                aarnn_rust::generated_management::proto::MigrationAdvanceRequest {
                    principal_id: "operator".to_owned(),
                    transition_json: serde_json::to_string(&transition).unwrap(),
                    brain_id: 42,
                    group_update_json: String::new(),
                },
            ))
            .await
            .expect("migration phase transition")
            .into_inner();
        current = serde_json::from_str(&response.operation_json).unwrap();
    }
    assert_eq!(current.phase, MigrationPhase::Committed);

    let committed_error = service
        .cancel_migration(Request::new(MigrationCancelRequest {
            principal_id: "operator".to_owned(),
            brain_id: 42,
            operation_id: current.operation_id,
            observed_leader_term: LeaseTerm::INITIAL.raw(),
            expected_resource_version: current.resource_version,
            reason: "too late".to_owned(),
        }))
        .await
        .expect_err("committed migration cannot be cancelled")
        .code();
    assert_eq!(committed_error, tonic::Code::FailedPrecondition);

    // Keep the DTO exercised in this test as the exact local/remote CLI
    // interchange shape.
    let _: MigrationCancellation = serde_json::from_value(serde_json::json!({
        "operation_id": 1,
        "observed_leader_term": 1,
        "expected_resource_version": 1,
        "reason": "bounded"
    }))
    .unwrap();
}

#[tokio::test]
async fn secured_management_persists_operations_and_rejects_principal_spoofing() {
    use aarnn_rust::generated_management::proto::management_server::Management;
    let path = std::env::temp_dir().join(format!(
        "aarnn-secured-management-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut policy = Policy::default();
    policy.grant("operator", Capability::Operate);
    let first = SecuredManagementGrpcService::new(
        PersistedManagementOrchestrator::open(&path, LeaseTerm::INITIAL, policy.clone())
            .expect("persistent management state"),
    );
    let mut submit = Request::new(OperationRequest {
        principal_id: "operator".to_owned(),
        brain_id: "brain-a".to_owned(),
        kind: OperationKind::Start as i32,
        context: Some(context("secure-request", "secure-key", 1, 0)),
    });
    submit
        .extensions_mut()
        .insert(AuthenticatedPrincipal("operator".to_owned()));
    let accepted = first
        .submit_operation(submit)
        .await
        .expect("authenticated operation")
        .into_inner();
    drop(first);

    let second = SecuredManagementGrpcService::new(
        PersistedManagementOrchestrator::open(&path, LeaseTerm::INITIAL, policy)
            .expect("reopen persistent management state"),
    );
    let mut lookup = Request::new(aarnn_rust::generated_management::proto::OperationLookup {
        operation_id: accepted.operation_id,
        brain_id: "brain-a".to_owned(),
        observed_leader_term: 1,
    });
    lookup
        .extensions_mut()
        .insert(AuthenticatedPrincipal("operator".to_owned()));
    let recovered = second
        .get_operation(lookup)
        .await
        .expect("operation survives restart")
        .into_inner();
    assert_eq!(recovered.operation_id, accepted.operation_id);

    let mut spoofed = Request::new(OperationRequest {
        principal_id: "observer".to_owned(),
        brain_id: "brain-a".to_owned(),
        kind: OperationKind::Start as i32,
        context: Some(context("spoof-request", "spoof-key", 1, 1)),
    });
    spoofed
        .extensions_mut()
        .insert(AuthenticatedPrincipal("operator".to_owned()));
    let error = second
        .submit_operation(spoofed)
        .await
        .expect_err("principal spoof must be rejected");
    assert_eq!(error.code(), tonic::Code::PermissionDenied);
    std::fs::remove_file(&path).unwrap();
    let _ = std::fs::remove_file(path.with_extension("management.lock"));
}

#[tokio::test]
async fn secured_management_denies_unauthorised_reads_and_scopes_operation_lookup() {
    use aarnn_rust::generated_management::proto::management_server::Management;

    let path = std::env::temp_dir().join(format!(
        "aarnn-secured-management-read-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut policy = Policy::default();
    policy.grant("operator", Capability::Operate);
    let service = SecuredManagementGrpcService::new(
        PersistedManagementOrchestrator::open(&path, LeaseTerm::INITIAL, policy)
            .expect("persistent management state"),
    );

    let mut submit = Request::new(OperationRequest {
        principal_id: "operator".to_owned(),
        brain_id: "brain-a".to_owned(),
        kind: OperationKind::Start as i32,
        context: Some(context("read-request", "read-key", 1, 0)),
    });
    submit
        .extensions_mut()
        .insert(AuthenticatedPrincipal("operator".to_owned()));
    let operation = service
        .submit_operation(submit)
        .await
        .expect("operation accepted")
        .into_inner();

    let mut status = Request::new(StatusRequest {
        brain_id: "brain-a".to_owned(),
    });
    status
        .extensions_mut()
        .insert(AuthenticatedPrincipal("observer".to_owned()));
    assert_eq!(
        service
            .get_status(status)
            .await
            .expect_err("observer must not read status")
            .code(),
        tonic::Code::PermissionDenied
    );

    let mut lookup = Request::new(aarnn_rust::generated_management::proto::OperationLookup {
        operation_id: operation.operation_id,
        brain_id: "brain-a".to_owned(),
        observed_leader_term: 1,
    });
    lookup
        .extensions_mut()
        .insert(AuthenticatedPrincipal("observer".to_owned()));
    assert_eq!(
        service
            .get_operation(lookup)
            .await
            .expect_err("observer must not read another principal's operation")
            .code(),
        tonic::Code::PermissionDenied
    );

    let mut owner_lookup = Request::new(aarnn_rust::generated_management::proto::OperationLookup {
        operation_id: operation.operation_id,
        brain_id: "brain-a".to_owned(),
        observed_leader_term: 1,
    });
    owner_lookup
        .extensions_mut()
        .insert(AuthenticatedPrincipal("operator".to_owned()));
    assert!(service.get_operation(owner_lookup).await.is_ok());

    std::fs::remove_file(&path).unwrap();
    let _ = std::fs::remove_file(path.with_extension("management.lock"));
}

#[tokio::test]
async fn secured_management_refreshes_persisted_state_between_orchestrator_processes() {
    use aarnn_rust::generated_management::proto::management_server::Management;

    let path = std::env::temp_dir().join(format!(
        "aarnn-secured-management-refresh-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut policy = Policy::default();
    policy.grant("operator", Capability::Operate);
    policy.grant("operator", Capability::Read);
    let first = SecuredManagementGrpcService::new(
        PersistedManagementOrchestrator::open(&path, LeaseTerm::INITIAL, policy.clone())
            .expect("first orchestrator"),
    );
    let second = SecuredManagementGrpcService::new(
        PersistedManagementOrchestrator::open(&path, LeaseTerm::INITIAL, policy)
            .expect("second orchestrator"),
    );

    let mut submit = Request::new(OperationRequest {
        principal_id: "operator".to_owned(),
        brain_id: "brain-refresh".to_owned(),
        kind: OperationKind::Start as i32,
        context: Some(context("refresh-request", "refresh-key", 1, 0)),
    });
    submit
        .extensions_mut()
        .insert(AuthenticatedPrincipal("operator".to_owned()));
    first
        .submit_operation(submit)
        .await
        .expect("operation accepted");

    let mut status = Request::new(StatusRequest {
        brain_id: "brain-refresh".to_owned(),
    });
    status
        .extensions_mut()
        .insert(AuthenticatedPrincipal("operator".to_owned()));
    let status = second
        .get_status(status)
        .await
        .expect("second process reads current state")
        .into_inner();
    assert_eq!(status.resource_version, 1);

    std::fs::remove_file(&path).unwrap();
    let _ = std::fs::remove_file(path.with_extension("management.lock"));
}

#[tokio::test]
async fn secured_management_requires_non_empty_brain_scope() {
    use aarnn_rust::generated_management::proto::management_server::Management;

    let path = std::env::temp_dir().join(format!(
        "aarnn-secured-management-empty-scope-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut policy = Policy::default();
    policy.grant("operator", Capability::Read);
    let service = SecuredManagementGrpcService::new(
        PersistedManagementOrchestrator::open(&path, LeaseTerm::INITIAL, policy)
            .expect("persistent management state"),
    );

    let mut status = Request::new(StatusRequest {
        brain_id: String::new(),
    });
    status
        .extensions_mut()
        .insert(AuthenticatedPrincipal("operator".to_owned()));
    assert_eq!(
        service
            .get_status(status)
            .await
            .expect_err("empty brain scope must be rejected")
            .code(),
        tonic::Code::InvalidArgument
    );

    std::fs::remove_file(&path).unwrap();
    let _ = std::fs::remove_file(path.with_extension("management.lock"));
}

#[test]
fn persisted_management_audit_is_hash_chained_and_tamper_evident() {
    let path = std::env::temp_dir().join(format!(
        "aarnn-secured-management-audit-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut policy = Policy::default();
    policy.grant("operator", aarnn_rust::management::Capability::Operate);
    let mut service = PersistedManagementOrchestrator::open(&path, LeaseTerm::INITIAL, policy)
        .expect("management state");
    service
        .submit_for_brain(
            aarnn_rust::management::Principal {
                id: "operator".to_owned(),
            },
            aarnn_rust::management::Capability::Operate,
            aarnn_rust::management::MutationContext {
                observed_leader_term: LeaseTerm::INITIAL,
                expected_version: 0,
                idempotency_key: "audit-key".to_owned(),
                request_id: "audit-request".to_owned(),
            },
            aarnn_rust::management::OperationKind::Start,
            "brain-a".to_owned(),
        )
        .expect("operation");
    assert!(service.state().verify_audit_integrity().is_ok());
    assert_eq!(service.state().audit()[0].sequence, 1);
    assert_eq!(service.state().audit()[0].digest.len(), 64);

    let mut document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    document["state"]["audit"][0]["outcome"] = serde_json::Value::String("forged".to_owned());
    std::fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
    let reopened =
        PersistedManagementOrchestrator::open(&path, LeaseTerm::INITIAL, Policy::default());
    assert!(matches!(
        reopened,
        Err(aarnn_rust::management::PersistedAuthorityError::Invalid(message))
            if message.contains("audit hash chain")
    ));
    std::fs::remove_file(&path).unwrap();
    let _ = std::fs::remove_file(path.with_extension("management.lock"));
}

#[tokio::test]
async fn secured_management_dispatch_is_async_and_idempotent() {
    use aarnn_rust::generated_management::proto::management_server::Management;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    let path = std::env::temp_dir().join(format!(
        "aarnn-secured-management-dispatch-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut policy = Policy::default();
    policy.grant("operator", Capability::Operate);
    let release = Arc::new(tokio::sync::Notify::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let dispatcher: ManagementOperationDispatcher = {
        let release = Arc::clone(&release);
        let calls = Arc::clone(&calls);
        Arc::new(move |_, _| {
            calls.fetch_add(1, Ordering::SeqCst);
            let release = Arc::clone(&release);
            Box::pin(async move {
                release.notified().await;
                Ok(())
            })
        })
    };
    let service = SecuredManagementGrpcService::with_dispatcher(
        PersistedManagementOrchestrator::open(&path, LeaseTerm::INITIAL, policy)
            .expect("persistent management state"),
        Some(dispatcher),
    );

    let request = || {
        let mut request = Request::new(OperationRequest {
            principal_id: "operator".to_owned(),
            brain_id: "brain-dispatch".to_owned(),
            kind: OperationKind::Start as i32,
            context: Some(context("dispatch-request", "dispatch-key", 1, 0)),
        });
        request
            .extensions_mut()
            .insert(AuthenticatedPrincipal("operator".to_owned()));
        request
    };
    let first = service
        .submit_operation(request())
        .await
        .expect("first operation")
        .into_inner();
    let duplicate = service
        .submit_operation(request())
        .await
        .expect("idempotent operation retry")
        .into_inner();
    assert_eq!(first.operation_id, duplicate.operation_id);
    assert_ne!(first.state, "succeeded");

    // The RPC has returned while the dispatcher is still blocked. Only one
    // retry may claim the durable operation and invoke the side effect.
    for _ in 0..64 {
        if calls.load(Ordering::SeqCst) == 1 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    release.notify_one();
    let mut terminal = None;
    for _ in 0..128 {
        let orchestrator_handle = service.orchestrator();
        let mut orchestrator = orchestrator_handle.lock().expect("management lock");
        orchestrator.refresh().expect("refresh management state");
        let state = orchestrator
            .operation(aarnn_rust::deterministic::EventId::new(first.operation_id).unwrap())
            .map(|operation| operation.state.clone());
        drop(orchestrator);
        if matches!(
            state,
            Some(aarnn_rust::management::OperationState::Succeeded)
        ) {
            terminal = state;
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(matches!(
        terminal,
        Some(aarnn_rust::management::OperationState::Succeeded)
    ));

    std::fs::remove_file(&path).unwrap();
    let _ = std::fs::remove_file(path.with_extension("management.lock"));
}

#[test]
fn management_takeover_requeues_interrupted_running_operations() {
    let path = std::env::temp_dir().join(format!(
        "aarnn-management-takeover-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut policy = Policy::default();
    policy.grant("operator", Capability::Operate);
    let mut old = PersistedManagementOrchestrator::open(&path, LeaseTerm::INITIAL, policy.clone())
        .expect("old leader state");
    let operation = old
        .submit_for_brain(
            aarnn_rust::management::Principal {
                id: "operator".to_owned(),
            },
            Capability::Operate,
            MutationContext {
                observed_leader_term: LeaseTerm::INITIAL,
                expected_version: 0,
                idempotency_key: "takeover-key".to_owned(),
                request_id: "takeover-request".to_owned(),
            },
            aarnn_rust::management::OperationKind::Start,
            "brain-takeover".to_owned(),
        )
        .expect("operation accepted");
    assert!(
        old.claim_pending(operation.id, LeaseTerm::INITIAL)
            .expect("old leader claims operation")
    );
    drop(old);

    let recovered =
        PersistedManagementOrchestrator::open(&path, LeaseTerm::new(2).unwrap(), policy)
            .expect("new leader state");
    assert_eq!(recovered.state().leader_term(), LeaseTerm::new(2).unwrap());
    assert!(matches!(
        recovered.state().operation(operation.id).unwrap().state,
        aarnn_rust::management::OperationState::Pending
    ));
    assert!(
        recovered
            .state()
            .audit()
            .iter()
            .any(|record| record.outcome == "requeued-after-leader-takeover")
    );

    std::fs::remove_file(&path).unwrap();
    let _ = std::fs::remove_file(path.with_extension("management.lock"));
}

#[tokio::test]
async fn management_apply_dispatches_verified_activation_after_publication() {
    use aarnn_rust::generated_management::proto::management_server::Management;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let mut policy = Policy::default();
    policy.grant("operator", Capability::Operate);
    let calls = Arc::new(AtomicUsize::new(0));
    let dispatched = Arc::clone(&calls);
    let activation_dispatcher: PlacementActivationDispatcher = Arc::new(move |command| {
        dispatched.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            assert_eq!(command.target_node, "worker-a");
            assert_eq!(command.placement_idempotency_key, "placement-apply-key-1");
            Ok(())
        })
    });
    let service = ManagementGrpcService::with_dispatchers_and_activation(
        ManagementOrchestrator::new(LeaseTerm::INITIAL, policy),
        None,
        Some(activation_dispatcher),
    );
    let activation = aarnn_rust::stable_worker::StableWorkerActivationCommand::new(
        "activation-request",
        1,
        42,
        "brain-a",
        "worker-a",
        "{}",
    )
    .expect("activation command");

    let response = service
        .apply_placement(Request::new(
            aarnn_rust::generated_management::proto::PlacementCommandRequest {
                command_json: apply_placement_command_json(),
                cutover_json: String::new(),
                repartition_json: String::new(),
                stable_worker_activation_json: serde_json::to_string(&activation).unwrap(),
            },
        ))
        .await
        .expect("placement and activation dispatch")
        .into_inner();
    assert!(!response.receipt_json.is_empty());
    let registry: aarnn_rust::placement_registry::PlacementRegistry =
        serde_json::from_str(&response.registry_json).expect("registry JSON");
    assert_eq!(
        registry
            .activation_statuses
            .get("placement-apply-key-1")
            .map(|status| &status.state),
        Some(&aarnn_rust::placement_registry::PlacementActivationState::Queued)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    assert!(registry.active_plan.is_none());
    let receipt: aarnn_rust::placement_registry::PlacementApplyReceipt =
        serde_json::from_str(&response.receipt_json).expect("receipt JSON");
    assert!(!receipt.committed);
    let plan = registry.prepared_plan().cloned().expect("prepared plan");
    let shard_ids = plan
        .placements
        .iter()
        .map(|placement| placement.shard_id.raw())
        .collect::<Vec<_>>();
    let owned_shard_ids = plan
        .placements
        .iter()
        .filter(|placement| placement.active_node == "worker-a")
        .map(|placement| placement.shard_id.raw())
        .collect::<Vec<_>>();
    let application_acks = owned_shard_ids
        .iter()
        .map(
            |shard_id| aarnn_rust::stable_worker::StableShardApplicationAck {
                shard_id: *shard_id,
                brain_id: 42,
                topology_generation: plan.topology_generation.raw(),
                partition_generation: plan.partition_generation.raw(),
                plan_digest: plan.digest().to_string(),
                lease_term: plan.lease_term.raw(),
                fencing_token: plan.fencing_token,
                applied_tick: plan.effective_tag.tick,
                applied_microstep: plan.effective_tag.microstep,
                state_digest: "22".repeat(16),
                durable_wal_sequence: None,
                committed: true,
            },
        )
        .collect();
    let registration = aarnn_rust::stable_worker::StableWorkerRegistration {
        schema_version: aarnn_rust::stable_worker::STABLE_WORKER_REGISTRATION_SCHEMA_VERSION,
        profile: aarnn_rust::stable_worker::STABLE_EXECUTOR_PROFILE.to_owned(),
        network_id: "brain-a".to_owned(),
        brain_id: 42,
        topology_generation: plan.topology_generation.raw(),
        partition_generation: plan.partition_generation.raw(),
        topology_digest: "11".repeat(16),
        plan_digest: plan.digest().to_string(),
        shard_ids,
        owned_shard_ids,
        application_acks,
        lease_term: plan.lease_term.raw(),
        fencing_token: plan.fencing_token,
        current_tick: plan.effective_tag.tick,
        current_microstep: plan.effective_tag.microstep,
        state_digest: "33".repeat(16),
        max_input_events: 1,
        max_steps_per_poll: 1,
        authoritative: true,
    };
    assert_eq!(
        service
            .record_stable_worker_registration("worker-a", &registration)
            .expect("durable worker registration evidence"),
        1
    );
}
