package com.neuralmimicry.aarnn

/**
 * Generated from proto/management.proto; policy stays on the gateway.
 * management-schema-source-digest:bd43399d784f092c
 */
object GeneratedManagementClient {
    const val SCHEMA_VERSION: Int = 2
    const val SCHEMA_HEADER: String = "x-aarnn-management-schema"

    fun statusPath(brainId: String = ""): String =
        "/api/management/status?brain_id=${java.net.URLEncoder.encode(brainId, "UTF-8")}"

    fun workspacesPath(): String = "/api/runtime/workspaces"

    fun workspaceActivityPath(workspaceId: String, ownerId: String): String =
        "/api/runtime/workspaces/${encode(workspaceId)}/activity?owner=${encode(ownerId)}"

    fun workspaceTopologyPath(workspaceId: String, ownerId: String): String =
        "/api/runtime/workspaces/${encode(workspaceId)}/topology?owner=${encode(ownerId)}&max_nodes=512&max_edges=4096"

    fun workspaceControlPath(workspaceId: String): String =
        "/api/runtime/workspaces/${encode(workspaceId)}/control"

    fun updateNetworkPath(): String = "/api/update_network"

    fun controlNetworkPath(): String = "/api/control_network"

    fun aerInjectPath(): String = "/api/aer/inject"

    fun clusterSnapshotPath(networkId: String): String =
        "/api/cluster_snapshot?network_id=${encode(networkId)}"

    fun submitOperationPath(): String = "/api/management/operations"

    fun operationPath(operationId: Long, brainId: String, observedLeaderTerm: Long): String =
        "/api/operations/$operationId?brain_id=${encode(brainId)}&observed_leader_term=$observedLeaderTerm"

    fun submitMigrationPath(): String = "/api/management/migrations"

    fun advanceMigrationPath(): String = "/api/management/migrations/advance"

    fun migrationPath(operationId: Long, brainId: String, observedLeaderTerm: Long): String =
        "/api/management/migrations/$operationId?brain_id=${encode(brainId)}&observed_leader_term=$observedLeaderTerm"

    fun migrationStatusPath(operationId: Long, brainId: String, observedLeaderTerm: Long): String =
        migrationPath(operationId, brainId, observedLeaderTerm)

    fun cancelMigrationPath(): String = "/api/management/migrations/cancel"

    private fun encode(value: String): String =
        java.net.URLEncoder.encode(value, "UTF-8")
}
