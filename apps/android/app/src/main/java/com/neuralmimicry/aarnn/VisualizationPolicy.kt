package com.neuralmimicry.aarnn

/** Shared nine-stage presentation policy. It never changes network stimuli. */
internal object VisualizationPolicy {
    const val VERSION = 1
    const val UPSHIFT_DWELL_MS = 1_500L
    val stages = listOf(
        "Synthetic pixels", "Synthetic connections", "Synthetic neurons",
        "Anatomical pixels", "Straight connections", "Branching connections",
        "Anatomical centre-lines", "Anatomical volumes", "Synapses and boutons",
    )
    private val zoomUpperBounds = doubleArrayOf(0.45, 0.65, 0.90, 1.20, 1.55, 1.95, 2.50, 3.20)
    private val latencyUpperBoundsMs = doubleArrayOf(5.0, 8.0, 12.0, 16.0, 24.0, 33.0, 50.0, 80.0)

    fun zoomStage(zoom: Double): Int = if (!zoom.isFinite()) 1 else (zoomUpperBounds.indexOfFirst { zoom < it } + 1).let { if (it == 0) 9 else it }

    fun latencyStage(latencyMs: Double): Int {
        if (!latencyMs.isFinite()) return 1
        latencyUpperBoundsMs.forEachIndexed { index, bound -> if (latencyMs <= bound) return 9 - index }
        return 1
    }

    fun update(current: Int, zoom: Double, latencyMs: Double, highest: Int, dwellMs: Long): Int {
        val currentStage = current.coerceIn(1, 9)
        val maximum = highest.coerceIn(1, 9)
        val target = minOf(zoomStage(zoom), latencyStage(latencyMs), maximum)
        return when {
            target < currentStage -> target
            target > currentStage && dwellMs >= UPSHIFT_DWELL_MS -> currentStage + 1
            else -> minOf(currentStage, maximum)
        }
    }

    fun highestAvailable(synthetic: RemoteDisplaySnapshot?, anatomical: RemoteDisplaySnapshot?): Int {
        if (anatomical == null || anatomical.nodes.isEmpty()) {
            return if (synthetic?.nodes?.isNotEmpty() == true) 3 else 1
        }
        var highest = if (anatomical.edges.any { !it.isPath }) 6 else 4
        if (anatomical.volumetricClearanceVerified && anatomical.edges.any { it.isPath }) {
            val somasHavePhysicalRadius = anatomical.nodes.all { (it.somaRadiusMM ?: 0.0) > 0.0 }
            val pathsHavePhysicalRadius = anatomical.edges.filter { it.isPath }.all { it.radius.isFinite() && it.radius > 0.0 }
            if (somasHavePhysicalRadius && pathsHavePhysicalRadius) {
                // A clearance flag without physical volume radii cannot prove
                // that the 3D scene is free of overlapping items.
                highest = 8
                if (anatomical.markers.isNotEmpty()) highest = 9
            }
        }
        return highest
    }
}

/** Immutable display policy captured for one FPV camera waypoint. */
data class FpvVisualizationKeyframe(
    val waypointId: String,
    val automatic: Boolean,
    val stage: Int,
    val zoom: Double,
)

/** Thread-safe bounded p95 sampler for renderer-only elapsed time. */
internal class VisualizationLatencyTracker(private val capacity: Int = 32) {
    private val samples = ArrayDeque<Double>()

    @Synchronized
    fun record(milliseconds: Double) {
        if (!milliseconds.isFinite() || milliseconds < 0.0) return
        samples.addLast(milliseconds)
        while (samples.size > capacity.coerceAtLeast(1)) samples.removeFirst()
    }

    @Synchronized
    fun p95Milliseconds(): Double {
        if (samples.isEmpty()) return 16.0
        val ordered = samples.sorted()
        return ordered[((ordered.size - 1) * 95) / 100]
    }
}
