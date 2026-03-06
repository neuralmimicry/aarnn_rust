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

## Secrets and Tokens
- Avoid embedding secrets in images or config files.
- Prefer environment variables or secret managers for `NM_OIDC_CLIENT_SECRET`.

## Transport Security
- Run the web UI behind TLS (reverse proxy or load balancer).
- gRPC endpoints should be protected by mTLS or an authenticated network boundary.

## Logging
- Operational logs and audit records should be centralized and retained per policy.
- Avoid logging raw tokens or PII.

## Operational Requirements (Outside This Repo)
- Access reviews, least privilege, and credential rotation.
- Vulnerability scanning and patching SLAs.
- Incident response and backup/retention procedures.
