# Compliance Posture (ISO 27001 / SOC 2)

This project includes security controls that support ISO 27001 and SOC 2 alignment. It does not, by itself, confer certification. Compliance requires organizational policies, audits, and evidence collection.

## Scope
- Web UI server (`src/bin/web_ui.rs`) and session management.
- Distributed orchestration endpoints and data paths.
- CLI/UI runners and local configuration.

## Control Highlights
- Access control: configurable auth modes with OIDC support.
- Session security: TTL-based sessions for the web UI.
- Secure configuration: explicit auth mode selection; OIDC metadata validation.
- Operational logging: structured logs for auth events and system activity.

## ISO 27001 Alignment Notes
- Access control and identity: OIDC and local auth modes.
- Cryptography: TLS expected at the edge; secure secret handling recommended.
- Logging and monitoring: application logs can be integrated with SIEM.
- Secure development: configuration-driven controls and minimal default exposure.

## SOC 2 Trust Services Alignment Notes
- Security: authentication, authorization, and controlled access to endpoints.
- Availability: operational monitoring and deployment practices.
- Confidentiality: avoid secrets in configs; protect logs and metrics.
- Processing integrity: deterministic builds and configuration validation.

## Evidence Collection Suggestions
- Retain auth logs, configuration snapshots, and deployment records.
- Document access reviews and change management outside this repository.
