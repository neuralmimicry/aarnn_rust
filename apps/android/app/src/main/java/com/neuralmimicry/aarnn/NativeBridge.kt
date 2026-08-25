package com.neuralmimicry.aarnn

/**
 * Narrow JNI control seam. Native loading is optional in debug/reference
 * builds; an unavailable library is reported rather than emulated.
 */
object NativeBridge {
    @Volatile
    private var loaded = false

    init {
        loaded = runCatching {
            System.loadLibrary("aarnn_rust")
            true
        }.getOrDefault(false)
    }

    fun isAvailable(): Boolean = loaded

    fun abiVersion(): Int = if (loaded) nativeAbiVersion() else 0

    fun createStandalone(brainId: Long, specJson: String): Long =
        if (loaded) nativeCreate(brainId, specJson) else 0L

    fun restore(checkpoint: ByteArray): Long =
        if (loaded && checkpoint.size <= MAX_CHECKPOINT_BYTES) nativeRestore(checkpoint) else 0L

    fun initialise(handle: Long): Boolean = loaded && nativeInitialise(handle) == 0

    fun start(handle: Long): Boolean = loaded && nativeStart(handle) == 0

    fun pause(handle: Long): Boolean = loaded && nativePause(handle) == 0

    fun enterForeground(handle: Long): Boolean =
        loaded && nativeEnterForeground(handle) == 0

    fun enterBackground(handle: Long): ByteArray? =
        if (loaded) nativeEnterBackground(handle) else null

    fun step(handle: Long, input: ByteArray): Boolean =
        loaded && input.size <= MAX_INPUT_BYTES && nativeStep(handle, input) == 0

    fun checkpoint(handle: Long): ByteArray? =
        if (loaded) nativeCheckpoint(handle) else null

    fun destroy(handle: Long): Boolean = loaded && nativeDestroy(handle) == 0

    private external fun nativeAbiVersion(): Int
    private external fun nativeCreate(brainId: Long, specJson: String): Long
    private external fun nativeRestore(checkpoint: ByteArray): Long
    private external fun nativeInitialise(handle: Long): Int
    private external fun nativeStart(handle: Long): Int
    private external fun nativePause(handle: Long): Int
    private external fun nativeEnterForeground(handle: Long): Int
    private external fun nativeEnterBackground(handle: Long): ByteArray?
    private external fun nativeStep(handle: Long, input: ByteArray): Int
    private external fun nativeCheckpoint(handle: Long): ByteArray?
    private external fun nativeDestroy(handle: Long): Int

    private const val MAX_INPUT_BYTES = 64 * 1024
    private const val MAX_CHECKPOINT_BYTES = 16 * 1024 * 1024
}
