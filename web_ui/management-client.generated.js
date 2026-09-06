/* Generated from proto/management.proto by the management contract build.
 * management-schema-source-digest:bd43399d784f092c
 * Do not add policy or direct-worker calls here; the gateway remains the
 * authorisation and fencing boundary. */
(function (global) {
  "use strict";
  const SCHEMA_VERSION = 2;
  class AarnnGeneratedManagementClient {
    constructor(fetchImpl) {
      this.fetchImpl = fetchImpl || global.fetch.bind(global);
    }

    request(path, options) {
      const request = Object.assign({}, options || {});
      const headers = new Headers(request.headers || {});
      headers.set("x-aarnn-management-schema", String(SCHEMA_VERSION));
      request.headers = headers;
      return this.fetchImpl(path, request);
    }

    status(brainId) {
      return this.request(`/api/management/status?brain_id=${encodeURIComponent(brainId || "")}`, { method: "GET" });
    }

    workspaces() {
      return this.request("/api/runtime/workspaces", { method: "GET" });
    }

    workspaceActivity(workspaceId, ownerId) {
      return this.request(`/api/runtime/workspaces/${encodeURIComponent(workspaceId || "")}/activity?owner=${encodeURIComponent(ownerId || "")}`, { method: "GET" });
    }

    workspaceTopology(workspaceId, ownerId, maxNodes, maxEdges) {
      return this.request(`/api/runtime/workspaces/${encodeURIComponent(workspaceId || "")}/topology?owner=${encodeURIComponent(ownerId || "")}&max_nodes=${encodeURIComponent(maxNodes || 512)}&max_edges=${encodeURIComponent(maxEdges || 4096)}`, { method: "GET" });
    }

    controlWorkspace(workspaceId, ownerId, action) {
      return this.request(`/api/runtime/workspaces/${encodeURIComponent(workspaceId || "")}/control`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ owner: ownerId || "", action: action || "" })
      });
    }

    updateNetwork(payload) {
      return this.request("/api/update_network", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(payload || {})
      });
    }

    controlNetwork(payload) {
      return this.request("/api/control_network", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(payload || {})
      });
    }

    injectAer(payload) {
      return this.request("/api/aer/inject", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(payload || {})
      });
    }

    submitOperation(principalId, brainId, kind, context) {
      return this.request("/api/management/operations", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ principal_id: principalId || "", brain_id: brainId || "", kind: kind || "", context: context || {} })
      });
    }

    clusterSnapshot(networkId) {
      return this.request(`/api/cluster_snapshot?network_id=${encodeURIComponent(networkId || "")}`, { method: "GET" });
    }

    operation(operationId, brainId, observedLeaderTerm) {
      return this.request(`/api/operations/${encodeURIComponent(operationId)}?brain_id=${encodeURIComponent(brainId || "")}&observed_leader_term=${encodeURIComponent(observedLeaderTerm || 0)}`, { method: "GET" });
    }

    submitMigration(payload) {
      return this.request("/api/management/migrations", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(payload || {})
      });
    }

    advanceMigration(payload) {
      return this.request("/api/management/migrations/advance", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(payload || {})
      });
    }

    migration(operationId, brainId, observedLeaderTerm) {
      return this.request(`/api/management/migrations/${encodeURIComponent(operationId)}?brain_id=${encodeURIComponent(brainId || "")}&observed_leader_term=${encodeURIComponent(observedLeaderTerm || 0)}`, { method: "GET" });
    }

    migrationStatus(operationId, brainId, observedLeaderTerm) {
      return this.request(`/api/management/migrations/${encodeURIComponent(operationId)}?brain_id=${encodeURIComponent(brainId || "")}&observed_leader_term=${encodeURIComponent(observedLeaderTerm || 0)}`, { method: "GET" });
    }

    cancelMigration(payload) {
      return this.request("/api/management/migrations/cancel", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(payload || {})
      });
    }
  }
  global.AARNNGeneratedManagementClient = AarnnGeneratedManagementClient;
  global.AARNN_MANAGEMENT_SCHEMA_VERSION = SCHEMA_VERSION;
})(window);
