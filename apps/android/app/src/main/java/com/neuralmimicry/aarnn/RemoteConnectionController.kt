package com.neuralmimicry.aarnn

import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

enum class RemoteConnectionState { Idle, Connecting, Connected, Error }

data class RemoteConnectionUiState(
    val state: RemoteConnectionState = RemoteConnectionState.Idle,
    val snapshot: RemoteWorkspaceSnapshot? = null,
    val error: String? = null,
    val lastUpdatedMs: Long? = null,
)

/** Keeps all network work off Compose and Android's main thread. */
class RemoteConnectionController : AutoCloseable {
    var uiState by mutableStateOf(RemoteConnectionUiState())
        private set

    private val executor: ExecutorService = Executors.newSingleThreadExecutor()
    private val main = Handler(Looper.getMainLooper())
    private var client: RemoteAarnnClient? = null
    private var refreshScheduled = false

    fun connect(endpoint: String, virtualHost: String, username: String, password: String) {
        if (endpoint.isBlank() || virtualHost.isBlank() || username.isBlank() || password.isBlank()) {
            uiState = uiState.copy(state = RemoteConnectionState.Error, error = "Endpoint, host, user and password are required")
            return
        }
        disconnect(clearError = false)
        uiState = RemoteConnectionUiState(state = RemoteConnectionState.Connecting)
        executor.execute {
            runCatching {
                val nextClient = RemoteAarnnClient(endpoint.trim(), virtualHost.trim())
                nextClient.login(username.trim(), password)
                val snapshot = nextClient.loadWorkspace(PREFERRED_WORKSPACE)
                client = nextClient
                post {
                    uiState = uiState.copy(
                        state = RemoteConnectionState.Connected,
                        snapshot = snapshot,
                        error = null,
                        lastUpdatedMs = System.currentTimeMillis(),
                    )
                    scheduleRefresh()
                }
            }.onFailure { error ->
                post {
                    uiState = uiState.copy(
                        state = RemoteConnectionState.Error,
                        error = error.message ?: "Connection failed",
                    )
                }
            }
        }
    }

    fun refresh() {
        executor.execute {
            val activeClient = client ?: return@execute
            val workspaceId = uiState.snapshot?.summary?.workspaceId
            runCatching { activeClient.loadWorkspace(workspaceId) }
                .onSuccess { snapshot ->
                    post {
                        uiState = uiState.copy(
                            state = RemoteConnectionState.Connected,
                            snapshot = snapshot,
                            error = null,
                            lastUpdatedMs = System.currentTimeMillis(),
                        )
                    }
                }
                .onFailure { error ->
                    post { uiState = uiState.copy(error = error.message ?: "Refresh failed") }
                }
        }
    }

    fun disconnect(clearError: Boolean = true) {
        client = null
        refreshScheduled = false
        main.removeCallbacksAndMessages(this)
        uiState = uiState.copy(
            state = RemoteConnectionState.Idle,
            snapshot = null,
            error = if (clearError) null else uiState.error,
            lastUpdatedMs = null,
        )
    }

    private fun scheduleRefresh() {
        if (refreshScheduled) return
        refreshScheduled = true
        main.postAtTime({
            refreshScheduled = false
            if (uiState.state == RemoteConnectionState.Connected) {
                refresh()
                scheduleRefresh()
            }
        }, this, System.currentTimeMillis() + REFRESH_INTERVAL_MS)
    }

    private fun post(action: () -> Unit) = main.post(action)

    override fun close() {
        disconnect()
        executor.shutdownNow()
    }

    companion object {
        private const val REFRESH_INTERVAL_MS = 2_000L
        private const val PREFERRED_WORKSPACE = "neuralmimicry-shared-snn"
    }
}
