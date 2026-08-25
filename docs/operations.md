# Operations

## Training

Run a local training pass:

```bash
scripts/train.sh
```

The output model artefact is written to `ml/models/demo_model.json`.

## Serving

Start the local API:

```bash
python -m uvicorn ml.inference.server:app --host 0.0.0.0 --port 8000
```

Health check:

```bash
curl http://127.0.0.1:8000/healthz
```

Prediction:

```bash
curl -X POST http://127.0.0.1:8000/predict \
  -H 'content-type: application/json' \
  -d '{"inputs":[0.1,0.2,0.3]}'
```

## Deployment

Apply the desired overlay:

```bash
scripts/deploy.sh canary
scripts/deploy.sh prod
```

## Hybrid k3s and native AARNN nodes

Cluster membership is held by the orchestrator process. Run exactly one
orchestrator replica and make its gRPC address routable from native workers. For
k3s, Kubernetes service-link variables are detected automatically inside pods;
outside Kubernetes, configure workers explicitly:

```bash
NM_ORCHESTRATOR_ADDRS=http://192.168.1.61:50051
NM_ADVERTISE_ADDR=192.168.1.60:50051
NM_NODE_ID=native-qc00
NM_PRELOAD_NODE_NETWORK=0
```

`NM_ORCHESTRATOR_ADDRS` accepts comma-, semicolon-, or whitespace-separated
endpoints in preference order. Singular `NM_ORCHESTRATOR_ADDR` and the equivalent
`AARNN_*` names remain supported. Connection and RPC bounds can be adjusted with
`NM_ORCHESTRATOR_CONNECT_TIMEOUT_MS` and `NM_ORCHESTRATOR_RPC_TIMEOUT_MS`.
Set `NM_NODE_ID` (or `--node-id`) to a stable, host-unique value so reconnects
replace the same membership entry and UI node labels remain recognisable.

If every endpoint is unavailable, a worker listens for the orchestrator UDP
beacon on port 50050 and keeps retrying rather than exiting. Broadcast and
loopback discovery remain enabled; set `NM_DISCOVERY_TARGETS` on the orchestrator
to add unicast targets across k3s/LAN boundaries. Set `NM_ADVERTISE_ADDR` whenever
the gRPC bind address is wildcarded or NAT could otherwise make the registered
worker address ambiguous.

Distributed spike streams are peer-to-peer. Kubernetes engine pods must
therefore advertise addresses that native workers can route to as well; merely
making the orchestrator reachable is insufficient. The SwarmHPC deployment runs
engine pods on their node networks at TCP 50052, separate from the orchestrator
on TCP 50051.

The causal migration contracts are currently reference-only and default-off.
The generated causal service has a bounded validation/echo implementation for
contract testing, not a production shard receiver or durable acknowledgement
boundary.
Do not enable `causal_transport`, `replicated_durability`, `management_v1` or
`workstation_io` in a deployment until the corresponding generated protocol,
quorum, durable-store and browser/native-device gates have passed. Existing
layer/vector `SpikeBatch` traffic is compatibility behaviour and is not a
claim of causally coherent distributed execution.

Useful checks:

```bash
kubectl -n aarnn get deployment aarnn-orchestrator \
  -o jsonpath='{.spec.replicas}{"\n"}'
kubectl -n aarnn logs deployment/aarnn-orchestrator --tail=200

systemctl status aarnn-node
journalctl -u aarnn-node -n 100 --no-pager
ss -lntup | grep -E ':50050|:50051|:50052'
```

For authorised read-only workspace observation, the gateway also provides:

```text
GET /api/runtime/workspaces/{workspace_id}/topology?max_nodes=512&max_edges=4096
```

The response is bounded and carries `topology_generation`, layer/node metadata,
active state and exact non-zero weighted edges for the included nodes. It is a
local-runner compatibility projection until the distributed shard snapshot RPC
and generated management clients are accepted; it must not be treated as
cluster-global topology evidence.

The web UI and Rust UI connect to the orchestrator, not independently to every
worker. Their status/node selectors therefore include native nodes as soon as
those nodes join and disappear after stale membership is pruned.

## Continuum Autoscaler + Tracey Recruit

When runtime autoscaling is enabled, AARNN sends a Tracey recruit block with every
`/node/recruit` request. This is required by Continuum environments that enforce
Tracey metadata.

Required runtime env:

```bash
NM_RUNTIME_CONTINUUM_URL
NM_RUNTIME_CONTINUUM_HOSTS
```

Tracey defaults (applied when not overridden):

```bash
NM_RUNTIME_CONTINUUM_TRACEY_AGENT_PREFIX=aarnn
NM_RUNTIME_CONTINUUM_TRACEY_AUTO_DISCOVERY=1
```

Optional Tracey override:

```bash
NM_RUNTIME_CONTINUUM_TRACEY_STATUS_ADDR=http://<host>:<port>
```

Quick runtime verification:

```bash
curl -s -c /tmp/aarnn.cookies -H 'Content-Type: application/json' \
  -d '{"username":"<user>","password":"<pass>"}' \
  http://<aarnn-web-ui>/api/login >/dev/null

curl -s -b /tmp/aarnn.cookies http://<aarnn-web-ui>/api/runtime/status
```

Expect autoscaler fields to show:
- `"enabled": true`
- non-empty `"last_action"` with recruit success
- `"cluster_nodes"` > 1 after remote recruit

Telemetry warning interpretation:
- If `"cluster_nodes"` is already > 1 and workspace distribution shows multiple
  nodes, a `last_action` value like
  `"cluster telemetry unavailable: failed to connect to orchestrator for autoscaler telemetry"`
  can be stale from an earlier transient outage.
- On older images, restart `deployment/aarnn-web-ui` to clear that stale warning.
- Runtime code now clears this message automatically after telemetry recovers
  (`src/runtime.rs`, `clear_stale_cluster_telemetry_error`).

## Authenticated Web UI Workspace Flow

In authenticated mode, the `NETWORK` and `NODE` selectors are driven by
`/api/runtime/status` workspace summaries. If status latency is higher than the
poll interval, overlapping status polls can prevent workspace state from
settling unless requests are serialized.

Runtime checks:

```bash
curl -s -c /tmp/aarnn.cookies -H 'Content-Type: application/json' \
  -d '{"username":"<user>","password":"<pass>"}' \
  https://aarnn.neuralmimicry.ai/api/login >/dev/null

curl -s -b /tmp/aarnn.cookies https://aarnn.neuralmimicry.ai/api/runtime/status \
  | jq '{autoscaler:.autoscaler,workspaces:.workspaces}'
```

UI checks after login:
- `NETWORK` is enabled when workspace summaries exist.
- Selecting `system::neuralmimicry-shared-snn` updates namespace label to `system`.
- `NODE` includes `All nodes` plus distributed node IDs for the selected workspace.

## Promotion

Use the CI workflow dispatch to promote an immutable build into an environment/track alias.
