package com.neuralmimicry.aarnn

import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL
import java.net.URLEncoder
import org.json.JSONArray
import org.json.JSONObject

/**
 * Small, read-only client for the authorised workspace projection exposed by
 * the Rust web gateway. It never dials workers or the raw orchestrator gRPC
 * port. HTTP is enabled here only for the local emulator validation target;
 * production deployments must use HTTPS.
 */
class RemoteAarnnClient(
    private val baseUrl: String,
    private val virtualHost: String,
) {
    private var sessionCookie: String? = null

    fun login(username: String, password: String) {
        val body = JSONObject().put("username", username).put("password", password)
        val response = request("/api/login", "POST", body.toString())
        val login = JSONObject(response.body)
        if (!login.optBoolean("ok") || !login.optBoolean("authenticated")) {
            throw RemoteAarnnException("Login was not accepted")
        }
    }

    fun loadWorkspace(preferredWorkspaceId: String? = null): RemoteWorkspaceSnapshot {
        val workspaces = parseWorkspaces(request("/api/runtime/workspaces", "GET").body)
        if (workspaces.isEmpty()) throw RemoteAarnnException("No authorised workspaces were returned")
        val summary = workspaces.firstOrNull { it.workspaceId == preferredWorkspaceId }
            ?: workspaces.first()
        val owner = encode(summary.ownerId.ifBlank { "system" })
        val workspace = encode(summary.workspaceId)
        val activity = parseActivity(
            request("/api/runtime/workspaces/$workspace/activity?owner=$owner", "GET").body,
        )
        val topology = try {
            parseTopology(
                request(
                    "/api/runtime/workspaces/$workspace/topology?owner=$owner&max_nodes=512&max_edges=4096",
                    "GET",
                ).body,
            )
        } catch (error: RemoteAarnnException) {
            // Older gateways may not expose the additive route yet. Keep the
            // authorised activity view usable, but make the missing topology
            // explicit so the UI cannot present fabricated live edges.
            if (error.message?.startsWith("HTTP 404") == true) {
                RemoteTopology.unavailable()
            } else {
                throw error
            }
        }
        return RemoteWorkspaceSnapshot(summary, activity, topology)
    }

    private fun request(path: String, method: String, body: String? = null): RemoteResponse {
        val connection = (URL(baseUrl.trimEnd('/') + path).openConnection() as HttpURLConnection).apply {
            requestMethod = method
            connectTimeout = CONNECT_TIMEOUT_MS
            readTimeout = READ_TIMEOUT_MS
            useCaches = false
            doInput = true
            setRequestProperty("Accept", "application/json")
            // The ingress is selected by this host while the emulator dials
            // the explicitly configured workstation IP.
            setRequestProperty("Host", virtualHost)
            sessionCookie?.let { setRequestProperty("Cookie", it) }
        }
        try {
            if (body != null) {
                connection.doOutput = true
                connection.setRequestProperty("Content-Type", "application/json")
                connection.outputStream.use { it.write(body.toByteArray(Charsets.UTF_8)) }
            }
            val status = connection.responseCode
            val stream = if (status in 200..299) connection.inputStream else connection.errorStream
            val responseBody = stream?.use { readBounded(it, MAX_RESPONSE_BYTES) }.orEmpty()
            if (status !in 200..299) {
                throw RemoteAarnnException(
                    "HTTP $status" + responseBody.takeIf { it.isNotBlank() }
                        ?.let { ": ${errorText(it)}" }.orEmpty(),
                )
            }
            connection.getHeaderField("Set-Cookie")?.let { cookie ->
                sessionCookie = cookie.substringBefore(';').takeIf { it.isNotBlank() }
            }
            return RemoteResponse(status, responseBody)
        } catch (error: RemoteAarnnException) {
            throw error
        } catch (error: IOException) {
            throw RemoteAarnnException("Connection failed: ${error.message ?: "I/O error"}", error)
        } finally {
            connection.disconnect()
        }
    }

    private fun parseWorkspaces(body: String): List<RemoteWorkspaceSummary> {
        val values = JSONArray(body)
        return buildList(values.length()) {
            for (index in 0 until values.length()) {
                val value = values.getJSONObject(index)
                add(
                    RemoteWorkspaceSummary(
                        ownerId = value.optString("owner_id", "system"),
                        workspaceId = value.optString("workspace_id"),
                        networkId = value.optString("network_id"),
                        name = value.optString("name"),
                        running = value.optBoolean("running"),
                        step = value.optLong("step"),
                        simTimeMs = value.optDouble("sim_time_ms"),
                        sensoryNeurons = value.optInt("num_sensory_neurons"),
                        hiddenLayers = value.optInt("num_hidden_layers"),
                        outputNeurons = value.optInt("num_output_neurons"),
                        totalNeurons = value.optInt("total_neurons"),
                        distributedNodeIds = value.optStringArray("distributed_node_ids"),
                    ),
                )
            }
        }
    }

    private fun parseActivity(body: String): RemoteActivity {
        val root = JSONObject(body).getJSONObject("activity")
        return RemoteActivity(
            step = root.optLong("step"),
            simTimeMs = root.optDouble("sim_time_ms"),
            sensory = root.optJSONArray("sensory")?.toIntList().orEmpty(),
            hidden = root.optJSONArray("hidden")?.toIntLists().orEmpty(),
            output = root.optJSONArray("output")?.toIntList().orEmpty(),
        )
    }

    private fun parseTopology(body: String): RemoteTopology {
        val root = JSONObject(body).getJSONObject("topology")
        val layers = root.optJSONArray("layers")?.let { values ->
            buildList(values.length()) {
                for (index in 0 until values.length()) {
                    val value = values.getJSONObject(index)
                    add(
                        RemoteTopologyLayer(
                            id = value.optString("id"),
                            name = value.optString("name"),
                            kind = value.optString("kind"),
                            neuronCount = value.optInt("neuron_count"),
                            visibleNodeCount = value.optInt("visible_node_count"),
                        ),
                    )
                }
            }
        }.orEmpty()
        val nodes = root.optJSONArray("nodes")?.let { values ->
            buildList(values.length()) {
                for (index in 0 until values.length()) {
                    val value = values.getJSONObject(index)
                    add(
                        RemoteTopologyNode(
                            id = value.optString("id"),
                            layerId = value.optString("layer_id"),
                            index = value.optInt("index"),
                            active = value.optBoolean("active"),
                        ),
                    )
                }
            }
        }.orEmpty()
        val edges = root.optJSONArray("edges")?.let { values ->
            buildList(values.length()) {
                for (index in 0 until values.length()) {
                    val value = values.getJSONObject(index)
                    add(
                        RemoteTopologyEdge(
                            sourceId = value.optString("source_id"),
                            targetId = value.optString("target_id"),
                            kind = value.optString("kind"),
                            weight = value.optDouble("weight"),
                        ),
                    )
                }
            }
        }.orEmpty()
        return RemoteTopology(
            schemaVersion = root.optInt("schema_version"),
            generation = root.optString("topology_generation"),
            step = root.optLong("step"),
            simTimeMs = root.optDouble("sim_time_ms"),
            layers = layers,
            nodes = nodes,
            edges = edges,
            totalNodeCount = root.optInt("total_node_count"),
            totalEdgeCount = root.optInt("total_edge_count"),
            truncated = root.optBoolean("truncated"),
        )
    }

    private fun errorText(body: String): String = runCatching {
        JSONObject(body).optString("error").ifBlank { "request rejected" }
    }.getOrDefault("request rejected")

    private fun encode(value: String): String = URLEncoder.encode(value, Charsets.UTF_8.name())

    companion object {
        private const val CONNECT_TIMEOUT_MS = 8_000
        private const val READ_TIMEOUT_MS = 15_000
        private const val MAX_RESPONSE_BYTES = 2 * 1024 * 1024
    }
}

data class RemoteWorkspaceSummary(
    val ownerId: String,
    val workspaceId: String,
    val networkId: String,
    val name: String,
    val running: Boolean,
    val step: Long,
    val simTimeMs: Double,
    val sensoryNeurons: Int,
    val hiddenLayers: Int,
    val outputNeurons: Int,
    val totalNeurons: Int,
    val distributedNodeIds: List<String>,
)

data class RemoteActivity(
    val step: Long,
    val simTimeMs: Double,
    val sensory: List<Int>,
    val hidden: List<List<Int>>,
    val output: List<Int>,
)

data class RemoteWorkspaceSnapshot(
    val summary: RemoteWorkspaceSummary,
    val activity: RemoteActivity,
    val topology: RemoteTopology,
)

data class RemoteTopology(
    val schemaVersion: Int,
    val generation: String,
    val step: Long,
    val simTimeMs: Double,
    val layers: List<RemoteTopologyLayer>,
    val nodes: List<RemoteTopologyNode>,
    val edges: List<RemoteTopologyEdge>,
    val totalNodeCount: Int,
    val totalEdgeCount: Int,
    val truncated: Boolean,
) {
    companion object {
        fun unavailable() = RemoteTopology(
            schemaVersion = 0,
            generation = "",
            step = 0,
            simTimeMs = 0.0,
            layers = emptyList(),
            nodes = emptyList(),
            edges = emptyList(),
            totalNodeCount = 0,
            totalEdgeCount = 0,
            truncated = false,
        )
    }
}

data class RemoteTopologyLayer(
    val id: String,
    val name: String,
    val kind: String,
    val neuronCount: Int,
    val visibleNodeCount: Int,
)

data class RemoteTopologyNode(
    val id: String,
    val layerId: String,
    val index: Int,
    val active: Boolean,
)

data class RemoteTopologyEdge(
    val sourceId: String,
    val targetId: String,
    val kind: String,
    val weight: Double,
)

class RemoteAarnnException(message: String, cause: Throwable? = null) : IOException(message, cause)

private data class RemoteResponse(val status: Int, val body: String)

private fun readBounded(stream: java.io.InputStream, limit: Int): String {
    val output = java.io.ByteArrayOutputStream()
    val buffer = ByteArray(8 * 1024)
    var total = 0
    while (true) {
        val count = stream.read(buffer)
        if (count < 0) break
        total += count
        if (total > limit) throw RemoteAarnnException("Response exceeds the $limit-byte limit")
        output.write(buffer, 0, count)
    }
    return output.toString(Charsets.UTF_8.name())
}

private fun JSONObject.optStringArray(name: String): List<String> {
    val values = optJSONArray(name) ?: return emptyList()
    return buildList(values.length()) {
        for (index in 0 until values.length()) add(values.optString(index))
    }
}

private fun JSONArray.toIntList(): List<Int> = buildList(length()) {
    for (index in 0 until length()) add(optInt(index))
}

private fun JSONArray.toIntLists(): List<List<Int>> = buildList(length()) {
    for (index in 0 until length()) add(optJSONArray(index)?.toIntList().orEmpty())
}
