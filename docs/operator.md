# Operator Resources

The operator-style control plane revolves around three CRDs:

- `Model`
- `Experiment`
- `Dataset`

These are represented as YAML examples under `operator/crds/`.

The Python controllers under `operator/controllers/` are functional sample reconcilers that:

- read a spec
- validate desired state
- derive runtime actions
- emit a reconciliation result
