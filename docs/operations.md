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
