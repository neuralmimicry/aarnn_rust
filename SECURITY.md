# Security Notes

This repository provides security controls that can support ISO 27001 and SOC 2 alignment. It does not, by itself, confer certification. Compliance depends on your operating environment, policies, and evidence collection.

## Scope
- Rust CLI and UI runner.
- Web UI server (`src/bin/web_ui.rs`) and its authentication flows.
- Distributed gRPC orchestration endpoints.

## Authentication and Access Control
- Web UI supports `auth_mode=none|local|oidc`.
- OIDC login uses the provider metadata and standard OAuth2/OIDC flows.
- For cross-project compatibility, the web UI also honors `NM_AUTH_MODE` and `NM_OIDC_*` environment variables when CLI flags are not provided.
- Browser-origin allowlists for cross-site auth discovery and OIDC handoff can be set with `NM_CORS_ORIGINS` (or `AARNN_CORS_ORIGINS`) as a comma-separated list. Leave it unset to keep those routes same-origin only.
- When auth is enabled, the browser and API no longer treat every authenticated session as equivalent. The web UI resolves the shared `service_access.aarnn` grant and enforces:
  - `request` for token and per-user configuration routes
  - `observe` for workspace status, detail, snapshot, activity, and export routes
  - `use` for workspace creation, import, and non-destructive control actions
  - `control` for destructive workspace actions such as delete, stop, reset, and new
- `POST /api/llm/mirror` requires `service_access.aarnn: use` and is intended for backend integrations such as Gail using Customers-issued bearer tokens.
- Local-only sessions keep explicit AARNN `control` access so standalone deployments do not lose existing runtime behaviour.
- Shared cluster-wide APIs remain blocked for authenticated sessions; authenticated users must use `/api/runtime/workspaces/*`.
- Customers-issued service-account sessions keep explicit groups only and must receive `service_access.aarnn` grants directly; they do not inherit the human authenticated fallback.

## Secrets and Tokens
- Avoid embedding secrets in images or config files.
- Prefer environment variables or secret managers for `NM_OIDC_CLIENT_SECRET`.
- Prefer Customers-issued service-account bearer tokens for backend routes such as `POST /api/llm/mirror`; do not reuse browser session cookies for inter-service traffic.

## Transport Security
- Run the web UI behind TLS (reverse proxy or load balancer).
- gRPC endpoints should be protected by mTLS or an authenticated network boundary.

## Logging
- Operational logs and audit records should be centralized and retained per policy.
- Avoid logging raw tokens or PII.
- Mirrored LLM exchanges accepted by `POST /api/llm/mirror` are persisted beneath `<runtime_root>/llm_mirror/`; apply retention, encryption, and access controls appropriate to mirrored prompt and response content.

## Operational Requirements (Outside This Repo)
- Access reviews, least privilege, and credential rotation.
- Vulnerability scanning and patching SLAs.
- Incident response and backup/retention procedures.
