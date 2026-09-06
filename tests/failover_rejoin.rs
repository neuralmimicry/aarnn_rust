use aarnn_rust::config::NetworkConfig;
use aarnn_rust::deterministic::ShardId;
use aarnn_rust::managed_durability::ManagedDurability;
use aarnn_rust::management::ReplicatedQuorumLeaseAuthority;
use aarnn_rust::recovery::{FileRecoveryEvidenceStore, ReplicaPlacement};
use aarnn_rust::runner::Runner;
use aarnn_rust::sim::{Learning, NeuronModel};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

const NETWORK_ID: &str = "process-failover-rejoin";
const ACTIVE_NODE: &str = "cp-a";
const REPLACEMENT_NODE: &str = "cp-b";
const MEMBERS: [&str; 3] = ["cp-a", "cp-b", "cp-c"];

fn runner() -> Runner {
    Runner::new(
        Default::default(),
        Default::default(),
        NetworkConfig::default(),
        NeuronModel::Lif,
        Learning::Stdp,
    )
}

fn replica_paths(root: &Path) -> Vec<(String, PathBuf)> {
    MEMBERS
        .iter()
        .map(|member| ((*member).to_owned(), root.join(format!("{member}.json"))))
        .collect()
}

fn wait_for(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {}",
            path.display()
        );
        std::thread::yield_now();
    }
}

fn child_config(root: &Path) -> (Vec<(String, PathBuf)>, Vec<String>) {
    let authority_root = root.join("authority");
    let replicas = replica_paths(&authority_root);
    let members = MEMBERS.iter().map(|member| (*member).to_owned()).collect();
    (replicas, members)
}

/// The child owns the active process boundary.  It deliberately stays alive
/// after the stale write attempt so the parent can inject a process kill after
/// fencing has been observed.
#[test]
fn child_process_owner() {
    let Some(root) = std::env::var_os("AARNN_FAILOVER_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let (replicas, members) = child_config(&root);
    let authority = ReplicatedQuorumLeaseAuthority::open(replicas.clone(), members.clone())
        .expect("child opens authority");
    let shard = aarnn_rust::managed_durability::managed_shard_id(NETWORK_ID);
    let lease = authority
        .authority()
        .lease(shard)
        .cloned()
        .expect("parent issued active lease");

    let mut biological = runner();
    let mut owner = ManagedDurability::open(
        root.join("zone-a"),
        NETWORK_ID,
        ACTIVE_NODE,
        &biological,
        lease.term,
        Some(&root.join("zone-b")),
    )
    .expect("child opens active owner");
    owner.bind_replicated_authority(replicas, members);
    owner.set_fencing_token(lease.fencing_token);
    biological.step(None);
    owner
        .commit_runner_step(&biological)
        .expect("first child commit");
    let committed = owner
        .authoritative_snapshot()
        .expect("snapshot")
        .expect("committed snapshot");
    fs::write(root.join("committed.snapshot"), committed).expect("publish child marker");

    wait_for_child_release(&root.join("release.stale"));
    biological.step(None);
    let stale_rejected = owner.commit_runner_step(&biological).is_err();
    fs::write(root.join("stale.result"), stale_rejected.to_string()).expect("publish fence result");

    // The parent kills this process to model loss of the active node after
    // the authority has fenced it.  Keeping the loop here makes the kill
    // boundary deterministic instead of racing process exit with recovery.
    loop {
        std::thread::park();
    }
}

fn wait_for_child_release(path: &Path) {
    wait_for(path);
}

fn start_child(root: &Path) -> Child {
    Command::new(std::env::current_exe().expect("integration test executable"))
        .args(["--exact", "child_process_owner", "--nocapture"])
        .env("AARNN_FAILOVER_ROOT", root)
        .spawn()
        .expect("spawn active owner process")
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn cross_process_failover_fences_killed_owner_and_rejoins_as_warm() {
    let root = std::env::temp_dir().join(format!(
        "aarnn-process-failover-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    fs::create_dir_all(root.join("authority")).expect("create authority root");
    let replicas = replica_paths(&root.join("authority"));
    let members = MEMBERS
        .iter()
        .map(|member| (*member).to_owned())
        .collect::<Vec<_>>();
    let mut authority = ReplicatedQuorumLeaseAuthority::open(replicas.clone(), members.clone())
        .expect("open replicated authority");
    let shard: ShardId = aarnn_rust::managed_durability::managed_shard_id(NETWORK_ID);
    let first = authority
        .issue_lease(shard, ACTIVE_NODE)
        .expect("issue active lease");

    let mut child = ChildGuard(start_child(&root));
    wait_for(&root.join("committed.snapshot"));
    let committed = fs::read_to_string(root.join("committed.snapshot")).expect("read committed");

    let replacement = authority
        .issue_lease(shard, REPLACEMENT_NODE)
        .expect("issue replacement lease");
    fs::write(root.join("release.stale"), b"release").expect("release stale attempt");
    wait_for(&root.join("stale.result"));
    assert_eq!(
        fs::read_to_string(root.join("stale.result")).expect("read fence result"),
        "true"
    );
    child.0.kill().expect("kill fenced active process");
    let _ = child.0.wait().expect("wait for killed process");

    // Remove only the resolved active-owner file.  The warm boundary and
    // quorum records remain durable and are the sole recovery inputs.
    let active_owner = root.join("zone-a").join(format!(
        "{}-{}-owner.json",
        "process-failover-rejoin", ACTIVE_NODE
    ));
    fs::remove_file(&active_owner).expect("remove failed active owner");

    let recovery_started = Instant::now();
    let mut recovered_runner = runner();
    let mut recovered = ManagedDurability::open(
        root.join("zone-b-recovered"),
        NETWORK_ID,
        REPLACEMENT_NODE,
        &recovered_runner,
        replacement.term,
        Some(&root.join("zone-b")),
    )
    .expect("recover replacement owner from warm state");
    recovered.bind_replicated_authority(replicas.clone(), members.clone());
    recovered.set_fencing_token(replacement.fencing_token);
    let recovered_snapshot = recovered
        .authoritative_snapshot()
        .expect("recovered snapshot")
        .expect("recovered biological state");
    assert_eq!(recovered_snapshot, committed);
    recovered_runner
        .import_network_json(&recovered_snapshot)
        .expect("restore replacement runner projection");
    recovered_runner.step(None);
    recovered
        .commit_runner_step(&recovered_runner)
        .expect("replacement commits after recovery");

    // A returning old node may restore bytes, but cannot rejoin as an active
    // writer while the authority lease belongs to the replacement.
    let mut old_runner = runner();
    let mut old_rejoin = ManagedDurability::open(
        root.join("zone-a-rejoin"),
        NETWORK_ID,
        ACTIVE_NODE,
        &old_runner,
        replacement.term,
        Some(&root.join("zone-b")),
    )
    .expect("old process can restore a warm copy");
    old_rejoin.bind_replicated_authority(replicas, members);
    old_rejoin.set_fencing_token(replacement.fencing_token);
    old_runner.step(None);
    assert!(old_rejoin.commit_runner_step(&old_runner).is_err());

    let observed_rto_ms = recovery_started.elapsed().as_millis().max(1) as u64;
    let durable = recovered
        .checkpoint_payload()
        .expect("recovered checkpoint");
    let warm = recovered
        .warm_checkpoint()
        .expect("warm checkpoint lookup")
        .expect("warm checkpoint");
    let evidence = aarnn_rust::recovery::RecoveryEvidenceBundle::from_durable_checkpoints(
        "cross-process-failover-rejoin",
        ReplicaPlacement {
            active_node: ACTIVE_NODE.to_owned(),
            active_failure_domain: "zone-a".to_owned(),
            warm_node: REPLACEMENT_NODE.to_owned(),
            warm_failure_domain: "zone-b".to_owned(),
        },
        first.term,
        replacement.term,
        &durable,
        &warm,
        0,
        observed_rto_ms,
        observed_rto_ms,
        true,
    )
    .expect("machine-verifiable recovery evidence");
    assert!(evidence.digest_verified);
    assert!(evidence.rpo_rto.as_ref().is_some_and(|rpo| rpo.pass));
    let store = FileRecoveryEvidenceStore::new(root.join("evidence")).expect("evidence store");
    store.publish(&evidence).expect("publish evidence");
    assert_eq!(
        store.load(&evidence.scenario_id).expect("reload evidence"),
        evidence
    );

    fs::remove_dir_all(root).expect("remove process test state");
}
