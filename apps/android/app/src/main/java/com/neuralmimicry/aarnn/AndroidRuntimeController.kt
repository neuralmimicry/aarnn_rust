package com.neuralmimicry.aarnn

/**
 * Owns the Android lifecycle around the narrow native seam.  In particular,
 * stopping the Activity never advances the biological clock: a native runtime
 * must publish a checkpoint before it is treated as backgrounded.
 */
class AndroidRuntimeController {
    enum class State { Unavailable, Available, Created, Ready, Running, Backgrounded, Terminated }

    var state: State = if (NativeBridge.isAvailable()) State.Available else State.Unavailable
        private set

    var lastCheckpoint: ByteArray? = null
        private set

    fun create(brainId: Long, specJson: String): Boolean {
        if (state != State.Available) return false
        val created = NativeBridge.createStandalone(brainId, specJson)
        if (created == 0L) return false
        handle = created
        state = State.Created
        return true
    }

    fun restore(checkpoint: ByteArray): Boolean {
        if (state != State.Available) return false
        val restored = NativeBridge.restore(checkpoint)
        if (restored == 0L) return false
        handle = restored
        state = State.Ready
        return true
    }

    fun initialise(): Boolean {
        if (state != State.Created) return false
        val ok = NativeBridge.initialise(handle)
        if (ok) state = State.Ready
        return ok
    }

    fun start(): Boolean {
        if (state != State.Ready) return false
        val ok = NativeBridge.start(handle)
        if (ok) state = State.Running
        return ok
    }

    fun onBackground(): Boolean {
        if (state != State.Running && state != State.Ready) return false
        val checkpoint = NativeBridge.enterBackground(handle) ?: return false
        lastCheckpoint = checkpoint
        state = State.Backgrounded
        return true
    }

    fun onForeground(): Boolean {
        if (state != State.Backgrounded) return false
        val ok = NativeBridge.enterForeground(handle)
        if (ok) state = State.Ready
        return ok
    }

    fun pause(): Boolean {
        if (state != State.Running) return false
        val ok = NativeBridge.pause(handle)
        if (ok) state = State.Ready
        return ok
    }

    fun close() {
        if (state == State.Unavailable || state == State.Available || state == State.Terminated) return
        NativeBridge.destroy(handle)
        state = State.Terminated
    }

    // Runtime creation is intentionally not automatic.  A future generated
    // binding will provide a validated EngineSpec and receive this handle.
    private var handle: Long = 0L
}
