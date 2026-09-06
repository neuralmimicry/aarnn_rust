//! Network-level verification for digest-bound stable-worker activation.
//!
//! The test deliberately uses the distributed gRPC service rather than
//! calling `DistributedNode::heartbeat` directly. It proves that a lost
//! heartbeat response does not lose an activation command and that an
//! acknowledged command is not replayed.

use aarnn_rust::distributed::DistributedNode;
use aarnn_rust::distributed::proto::distributed_neuromorphic_client::DistributedNeuromorphicClient;
use aarnn_rust::distributed::proto::distributed_neuromorphic_server::{
    DistributedNeuromorphic, DistributedNeuromorphicServer,
};
use aarnn_rust::distributed::proto::{
    HeartbeatRequest, JoinRequest, NetworkCommandResult, NetworkResources, Resources,
};
use aarnn_rust::managed_durability::managed_brain_id;
use tokio::sync::oneshot;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::Request;

async fn start_server(
    node: DistributedNode,
) -> (String, oneshot::Sender<()>, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("distributed listener");
    let address = listener.local_addr().expect("distributed address");
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(DistributedNeuromorphicServer::new(node))
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async move {
                let _ = shutdown_rx.await;
            })
            .await
            .expect("distributed server");
    });
    (format!("http://{address}"), shutdown_tx, task)
}

fn network_resources() -> std::collections::HashMap<String, NetworkResources> {
    std::collections::HashMap::from([(
        "alpha".to_owned(),
        NetworkResources {
            num_neurons: 2,
            layer_neuron_counts: std::collections::HashMap::from([(0, 2)]),
            avg_step_time_ms: 1.0,
        },
    )])
}

fn heartbeat(results: Vec<NetworkCommandResult>) -> HeartbeatRequest {
    HeartbeatRequest {
        node_id: "stable-worker".to_owned(),
        resources: Some(Resources::default()),
        network_resources: network_resources(),
        stable_executors: Vec::new(),
        stable_executor_capabilities: vec![
            aarnn_rust::distributed::proto::StableExecutorCapability {
                schema_version:
                    aarnn_rust::stable_worker::STABLE_EXECUTOR_CAPABILITY_SCHEMA_VERSION,
                profile: aarnn_rust::stable_worker::STABLE_EXECUTOR_PROFILE.to_owned(),
                activation_schema_version:
                    aarnn_rust::stable_worker::STABLE_WORKER_ACTIVATION_SCHEMA_VERSION,
                max_input_events: aarnn_rust::stable_worker::DEFAULT_STABLE_WORKER_MAX_INPUT_EVENTS,
                max_steps_per_poll:
                    aarnn_rust::stable_worker::DEFAULT_STABLE_WORKER_MAX_STEPS_PER_POLL,
            },
        ],
        command_results: results,
    }
}

#[tokio::test]
async fn activation_delivery_is_at_least_once_over_distributed_grpc() {
    let orchestrator = DistributedNode::new("orchestrator".to_owned(), true);
    let (address, shutdown, server) = start_server(orchestrator.clone()).await;
    let mut client = DistributedNeuromorphicClient::connect(address)
        .await
        .expect("distributed client");

    client
        .join(JoinRequest {
            node_id: "stable-worker".to_owned(),
            address: "http://127.0.0.1:65534".to_owned(),
            resources: Some(Resources::default()),
            network_resources: network_resources(),
            stable_executors: Vec::new(),
            stable_executor_capabilities: heartbeat(Vec::new()).stable_executor_capabilities,
        })
        .await
        .expect("worker join");

    let brain_id = managed_brain_id("alpha");
    let activation = aarnn_rust::stable_worker::StableWorkerActivationCommand::new(
        "grpc-activation",
        31,
        brain_id.raw(),
        "alpha",
        "stable-worker",
        "{}",
    )
    .expect("activation command");
    let manifest_digest = activation.manifest_digest.clone();
    orchestrator
        .queue_stable_worker_activation(activation)
        .await
        .expect("queue activation");

    let first = client
        .heartbeat(heartbeat(Vec::new()))
        .await
        .expect("first heartbeat")
        .into_inner();
    let activation_type =
        aarnn_rust::distributed::proto::network_command::CommandType::ActivateStableWorker as i32;
    let delivered = first
        .commands
        .iter()
        .find(|command| command.r#type == activation_type)
        .cloned()
        .expect("stable activation command delivered");

    let retry = client
        .heartbeat(heartbeat(Vec::new()))
        .await
        .expect("retry heartbeat")
        .into_inner();
    let retried = retry
        .commands
        .iter()
        .filter(|command| command.r#type == activation_type)
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(retried, vec![delivered]);

    let acknowledged = client
        .heartbeat(heartbeat(vec![NetworkCommandResult {
            command_type:
                aarnn_rust::distributed::proto::network_command::CommandType::ActivateStableWorker
                    as i32,
            network_id: "alpha".to_owned(),
            request_id: "grpc-activation".to_owned(),
            manifest_digest,
            accepted: true,
            error: String::new(),
            brain_id: brain_id.raw(),
            placement_idempotency_key: String::new(),
        }]))
        .await
        .expect("acknowledgement heartbeat")
        .into_inner();
    assert!(
        !acknowledged
            .commands
            .iter()
            .any(|command| command.r#type == activation_type)
    );

    let _ = shutdown.send(());
    tokio::time::timeout(std::time::Duration::from_secs(2), server)
        .await
        .expect("distributed server shutdown")
        .expect("distributed server task");
}
