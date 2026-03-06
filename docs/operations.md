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

## Promotion

Use the CI workflow dispatch to promote an immutable build into an environment/track alias.
