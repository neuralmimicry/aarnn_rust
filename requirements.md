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
{"action":"externalize_aarnn_sessions","finding_id":"473d733b-7cb5-470f-b4e4-692ce6e8883a","finding_key":"aarnn_singleton_web_ui"}

Planner guidance (advisory; it must not weaken or contradict the authoritative work-item requirements):
Overview: This work item addresses the architectural constraint of AARNN being a singleton web UI by externalising session and runtime coordination. The approach prioritises minimising risk through incremental refactoring, rigorous testing, and a canary rollout strategy to ensure operational resilience.

Requirements Register:
- REQ-001: Inspect repository state at /srv/neuralmimicry/aarnn_rust to identify singleton dependencies and shared state locks.
- REQ-002: Document observed evidence regarding session management bottlenecks and uncertainty levels in the current architecture.
- REQ-003: Formulate a hypothesis for the scaling limitation and propose a stateless session abstraction layer.
- REQ-004: Execute the smallest safe operation by refactoring the session handler to utilise an external store.
- REQ-005: Introduce a minimal regression test baseline to verify session persistence and isolation across multiple worker instances.
- REQ-006: Execute the new tests using cargo test to confirm the baseline is stable and non-destructive.
- REQ-007: Apply the code changes to the canary host 'spirit' using the Ansible playbook continuum_tenant_aarnn_site.yml.
- REQ-008: Monitor the canary rollout for a health window, verifying that session coordination functions correctly under load.
- REQ-009: Ensure automatic rollback on degradation if health checks fail during the canary phase.
- REQ-010: Verify rollback readiness and recovery procedures before finalising the deployment.
- REQ-011: Leave unrelated files untouched and avoid destructive commands during the implementation phase.
- REQ-012: Confirm post-rollout readiness health window is passed before marking the work item as complete.


Protected rollout contract (mandatory): capture a fresh readiness baseline before any change; use the selected canary or red_green strategy; verify health throughout the post-rollout window; if health or verification degrades, automatically revert the exact produced commit without rewriting history, rerun tests and GitHub Actions, and verify recovery.