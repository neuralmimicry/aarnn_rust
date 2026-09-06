//! Verification of the registered orchestrator migration dispatch seam.

use aarnn_rust::deterministic::{BrainId, LeaseTerm, LogicalTag, ShardId, StateDigest};
use aarnn_rust::generated_management::proto::{
    MigrationCommandRequest, MigrationLookup, management_server::Management,
};
use aarnn_rust::management::{Capability, ManagementGrpcService, ManagementOrchestrator, Policy};
use aarnn_rust::migration_executor::{
    MigrationDispatchReceipt, MigrationExecutor, MigrationExecutorRegistry,
};
use aarnn_rust::migration_group::{MigrationGroup, MigrationGroupSpec};
use aarnn_rust::migration_operation::{MigrationKind, MigrationOperation, MigrationRequest};
use std::sync::Arc;
use tonic::Request;

fn digest(byte: u8) -> StateDigest {
    StateDigest([byte; 16])
}

fn group_for(operation: &MigrationOperation, spec: &MigrationGroupSpec) -> MigrationGroup {
    let mut group = spec
        .build(operation.operation_id)
        .expect("valid group spec");
    for shard_id in spec.shard_ids.iter().copied() {
        group
            .begin_transfer(shard_id, spec.leader_term)
            .expect("begin transfer");
        group
            .mark_caught_up(
                shard_id,
                spec.leader_term,
                digest(shard_id.raw() as u8),
                LogicalTag::ZERO,
                LeaseTerm::new(spec.leader_term.raw() + 1).expect("destination term"),
                digest(3),
                digest(4),
            )
            .expect("mark caught up");
        group
            .mark_fenced(
                shard_id,
                spec.leader_term,
                LeaseTerm::new(spec.leader_term.raw() + 1).expect("destination term"),
            )
            .expect("mark fenced");
        group
            .mark_published(shard_id, spec.leader_term)
            .expect("mark published");
    }
    group.commit(spec.leader_term).expect("commit group");
    group
}

#[derive(Debug)]
struct ImmediateExecutor;

impl MigrationExecutor for ImmediateExecutor {
    fn execute(
        &self,
        operation: MigrationOperation,
        spec: MigrationGroupSpec,
    ) -> Result<MigrationDispatchReceipt, String> {
        Ok(MigrationDispatchReceipt {
            operation_id: operation.operation_id,
            brain_id: operation.brain_id,
            group: group_for(&operation, &spec),
            cut_tag: LogicalTag::ZERO,
            transferred_bytes: operation.progress.total_bytes,
        })
    }
}

fn service() -> ManagementGrpcService {
    let mut policy = Policy::default();
    policy.grant_for_brain("operator", "77", Capability::Operate);
    policy.grant_for_brain("operator", "77", Capability::Read);
    let orchestrator = ManagementOrchestrator::new(LeaseTerm::INITIAL, policy);
    let registry = MigrationExecutorRegistry::default();
    registry
        .register(BrainId::new(77).unwrap(), Arc::new(ImmediateExecutor))
        .expect("executor registration");
    ManagementGrpcService::with_migration_dispatcher(orchestrator, Some(registry.handler()))
}

fn request() -> MigrationRequest {
    MigrationRequest {
        request_id: "executor-request".to_owned(),
        idempotency_key: "executor-idempotency".to_owned(),
        brain_id: BrainId::new(77).unwrap(),
        observed_leader_term: LeaseTerm::INITIAL,
        expected_resource_version: 0,
        kind: MigrationKind::MigrateBrain,
        source_plan_digest: digest(1),
        target_plan_digest: digest(2),
        total_shards: 2,
        total_bytes: 128,
    }
}

fn spec() -> MigrationGroupSpec {
    MigrationGroupSpec {
        brain_id: BrainId::new(77).unwrap(),
        leader_term: LeaseTerm::INITIAL,
        topology_generation: aarnn_rust::deterministic::TopologyGeneration::INITIAL,
        partition_generation: aarnn_rust::deterministic::PartitionGeneration::INITIAL,
        shard_ids: vec![ShardId::new(10).unwrap(), ShardId::new(20).unwrap()],
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn registered_executor_dispatches_and_commits_group_evidence() {
    let service = service();
    let submitted = <ManagementGrpcService as Management>::submit_migration(
        &service,
        Request::new(MigrationCommandRequest {
            principal_id: "operator".to_owned(),
            command_json: serde_json::to_string(&request()).unwrap(),
            group_json: serde_json::to_string(&spec()).unwrap(),
        }),
    )
    .await
    .expect("migration submission")
    .into_inner();
    let operation: MigrationOperation = serde_json::from_str(&submitted.operation_json).unwrap();
    assert_eq!(
        operation.phase,
        aarnn_rust::migration_operation::MigrationPhase::Prepared
    );

    let mut committed = None;
    for _ in 0..128 {
        let response = <ManagementGrpcService as Management>::get_migration(
            &service,
            Request::new(MigrationLookup {
                brain_id: 77,
                operation_id: operation.operation_id,
                observed_leader_term: 1,
            }),
        )
        .await
        .expect("migration lookup")
        .into_inner();
        let current: MigrationOperation = serde_json::from_str(&response.operation_json).unwrap();
        if current.phase == aarnn_rust::migration_operation::MigrationPhase::Committed {
            committed = Some(current);
            break;
        }
        tokio::task::yield_now().await;
    }
    let committed = committed.expect("registered executor completed within bounded polling");
    assert_eq!(committed.progress.completed_shards, 2);
    assert_eq!(committed.progress.transferred_bytes, 128);
    assert_eq!(committed.progress.cut_tag, Some(LogicalTag::ZERO));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn registry_enforces_registration_and_releases_dispatch_lease() {
    let registry = MigrationExecutorRegistry::default();
    let brain_id = BrainId::new(77).unwrap();
    registry
        .register(brain_id, Arc::new(ImmediateExecutor))
        .unwrap();
    assert!(
        registry
            .register(brain_id, Arc::new(ImmediateExecutor))
            .is_err()
    );
    let receipt = registry
        .dispatch(
            MigrationOperation {
                operation_id: 1,
                request_id: "one".to_owned(),
                idempotency_key: "one".to_owned(),
                brain_id,
                kind: MigrationKind::MigrateBrain,
                source_plan_digest: digest(1),
                target_plan_digest: digest(2),
                phase: aarnn_rust::migration_operation::MigrationPhase::Prepared,
                progress: aarnn_rust::migration_operation::MigrationProgress {
                    completed_shards: 0,
                    total_shards: 2,
                    transferred_bytes: 0,
                    total_bytes: 128,
                    cut_tag: None,
                },
                resource_version: 1,
                error_code: None,
            },
            spec(),
        )
        .await;
    assert_eq!(receipt.unwrap().operation_id, 1);
    assert!(!registry.is_in_flight(brain_id, 1).unwrap());
    assert!(registry.unregister(brain_id).unwrap());
    let missing = registry
        .dispatch(
            MigrationOperation {
                operation_id: 2,
                request_id: "two".to_owned(),
                idempotency_key: "two".to_owned(),
                brain_id,
                kind: MigrationKind::MigrateBrain,
                source_plan_digest: digest(1),
                target_plan_digest: digest(2),
                phase: aarnn_rust::migration_operation::MigrationPhase::Prepared,
                progress: aarnn_rust::migration_operation::MigrationProgress {
                    completed_shards: 0,
                    total_shards: 2,
                    transferred_bytes: 0,
                    total_bytes: 128,
                    cut_tag: None,
                },
                resource_version: 1,
                error_code: None,
            },
            spec(),
        )
        .await
        .unwrap_err();
    assert!(missing.contains("is not registered"));
}
