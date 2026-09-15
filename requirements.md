Overview: Improve Webots through the Conductor execution loop.

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
- REQ-013: When runtime rollout or restart work is needed, use the available Ansible automation context: {"ansible_root":"/srv/swarmhpc/ansible","config_path":"/srv/swarmhpc/ansible/ansible.cfg","host_targets":["rk1"],"hosts":["spirit"],"inventory_path":"/srv/swarmhpc/ansible/inventory/hosts.ini","playbooks":["continuum_tenant_nmchain_site.yml","continuum_tenant_webots_site.yml"],"repo_root":"/srv/swarmhpc","roles_path":"/srv/swarmhpc/ansible/roles","secrets_root":"/srv/swarmhpc/ansible/.secrets"}.

Work Item Summary:
Webots depends on shared data services but no clear persistent-storage profile was inferred from Ansible. Confirm PVCs or durable mounts before further automation.

Authoritative delivery constraints (mandatory; implement and verify these, do not merely describe them):
- No structured delivery constraints were supplied; follow the work-item summary exactly.

Plan JSON:
{"action":"verify_persistent_storage","finding_id":"2eaecb12-2b80-4416-ab7f-345f32d9303f","finding_key":"storage_profile:webots","service":"webots"}

Planner guidance (advisory; it must not weaken or contradict the authoritative work-item requirements):
Overview: This work item verifies the persistent storage strategy for the Webots service to ensure data durability and state consistency. The operation focuses on inspecting the current repository and Ansible automation to confirm whether a clear persistent-storage profile exists. If gaps are identified, a minimal regression baseline will be established to enable safe autonomous changes without destructive modifications.

Requirements Register:
- REQ-001: Inspect repository state at /srv/neuralmimicry/aarnn_rust-webots for existing test coverage and runtime configurations.
- REQ-002: Document observed evidence regarding storage readiness and uncertainty levels in the repository inventory.
- REQ-003: Execute cargo fmt --check to ensure code style compliance before any compilation steps.
- REQ-004: Run cargo test to validate the current baseline stability without modifying production logic.
- REQ-005: Review Ansible playbooks for explicit PVC definitions or volume mount configurations.
- REQ-006: Probe the target host 'spirit' for existing persistent mounts or PVC bindings.
- REQ-007: Confirm whether durable storage is currently active or if a gap exists requiring remediation.
- REQ-008: Prepare a canary staged rollout plan only if the verification confirms a missing storage profile.
- REQ-009: Ensure no destructive commands are executed during the initial inspection phase.
- REQ-010: Leave unrelated files untouched and avoid bypassing staged delivery gates.
- REQ-011: Implement automatic rollback on degradation if the canary rollout impacts service health.
- REQ-012: Verify post-rollout readiness health window before marking the gap as closed.


Protected rollout contract (mandatory): capture a fresh readiness baseline before any change; use the selected canary or red_green strategy; verify health throughout the post-rollout window; if health or verification degrades, automatically revert the exact produced commit without rewriting history, rerun tests and GitHub Actions, and verify recovery.