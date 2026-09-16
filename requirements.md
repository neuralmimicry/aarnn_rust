Overview: Improve AARNN through the Conductor execution loop.

Delivery Context:
- Current stage: development
- Validated stages: none
- Rollout strategy: canary

Requirements Register:
- REQ-001: Inspect and record current repository, runtime, or job evidence before selecting an operation.
- REQ-002: Implement only the scoped change, job update, or progress-monitoring action supported by that evidence.
- REQ-003: Preserve secure, resilient behaviour and avoid destructive commands.
- REQ-004: Update or add tests covering the changed path, or provide the relevant live operational check.
- REQ-005: Run verification commands and report the outcome.
- REQ-006: Leave unrelated files untouched.
- REQ-007: Record rollback/recovery steps and the acceptance signal proving the gap is closed.
- REQ-008: Preserve staged progression and rollout governance metadata.
- REQ-009: Capture a fresh protected-target readiness baseline before any change.
- REQ-010: Use the selected canary or red-green rollout strategy and verify the post-rollout health window.
- REQ-011: Automatically revert the exact produced commit without rewriting history if health or verification degrades.
- REQ-012: Verify rollback readiness and recovery before finalising the delivery.
- REQ-013: When runtime rollout or restart work is needed, use the available Ansible automation context: {"ansible_root":"/srv/swarmhpc/ansible","config_path":"/srv/swarmhpc/ansible/ansible.cfg","host_targets":["rk1"],"hosts":["spirit"],"inventory_path":"/srv/swarmhpc/ansible/inventory/hosts.ini","playbooks":["continuum_tenant_aarnn_site.yml","continuum_tenant_nmchain_site.yml"],"repo_root":"/srv/swarmhpc","roles_path":"/srv/swarmhpc/ansible/roles","secrets_root":"/srv/swarmhpc/ansible/.secrets"}.

Work Item Summary:
AARNN is configured as a singleton web UI. Externalize session and runtime coordination before scaling the control surface horizontally.

Authoritative delivery constraints (mandatory; implement and verify these, do not merely describe them):
- No structured delivery constraints were supplied; follow the work-item summary exactly.

Plan JSON:
{"action":"externalize_aarnn_sessions","finding_id":"1ce9f0ad-0f8d-4f36-bdef-beee67e23d0c","finding_key":"aarnn_singleton_web_ui"}

Planner guidance (advisory; it must not weaken or contradict the authoritative work-item requirements):
Overview: This work item addresses the singleton configuration of AARNN, which limits horizontal scaling and introduces operational risk. The objective is to verify current runtime and repository state, confirm service health, and select the smallest safe operation to externalize session and runtime coordination.

Requirements Register:
- REQ-001: Inspect the current repository state at /srv/neuralmimicry/aarnn_rust to confirm path accessibility, branch state, and file structure.
- REQ-002: Query the local K3s cluster status using kubectl get nodes to verify control-plane connectivity and worker node probes.
- REQ-003: Review service health logs and metrics on host 'spirit' to identify specific degradation causes before proceeding with remediation.
- REQ-004: Execute ansible-playbook /srv/swarmhpc/ansible/playbooks/continuum_tenant_aarnn_site.yml --check to validate Ansible syntax and inventory without applying changes.
- REQ-005: Examine Conductor execution traces and alert history to understand the impact of the current singleton configuration on downstream decision-making.
- REQ-006: Assess the scope for externalizing session and runtime coordination based on verified evidence, prioritizing non-destructive job updates or configuration changes.
- REQ-007: Prepare a canary rollout strategy that monitors for degradation and triggers automatic rollback if key health signals are compromised.
- REQ-008: Execute the selected implementation steps, then perform post-rollout readiness checks to confirm stable operation before full promotion.

Notes:
- The repository path /srv/neuralmimicry/aarnn_rust must be accessible for source-driven improvements.
- Control-plane connectivity and worker node probes should be verified before attempting runtime modifications.
- The Ansible playbook syntax and inventory must be validated without applying changes to ensure safe execution.
- Service health logs and metrics on host 'spirit' are the primary source for identifying degradation causes.
- Conductor execution traces and alert history provide context for the impact of the current configuration.
- The scope for externalization should be limited to the smallest safe operation supported by current evidence.
- Non-destructive job updates or configuration changes are preferred over runtime intervention.
- A canary rollout strategy must include monitoring for degradation and automatic rollback capability.
- Post-rollout readiness checks should confirm stable operation before full promotion.


Protected rollout contract (mandatory): capture a fresh readiness baseline before any change; use the selected canary or red_green strategy; verify health throughout the post-rollout window; if health or verification degrades, automatically revert the exact produced commit without rewriting history, rerun tests and GitHub Actions, and verify recovery.