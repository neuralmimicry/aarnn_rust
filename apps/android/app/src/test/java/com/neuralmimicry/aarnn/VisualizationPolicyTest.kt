package com.neuralmimicry.aarnn

import org.junit.Assert.assertEquals
import org.junit.Test

class VisualizationPolicyTest {
    @Test
    fun latencyBandsMatchTheVersionedCrossProductPolicy() {
        val boundaries = listOf(5.0, 8.0, 12.0, 16.0, 24.0, 33.0, 50.0, 80.0)
        boundaries.forEachIndexed { index, boundary ->
            assertEquals(9 - index, VisualizationPolicy.latencyStage(boundary))
        }
        assertEquals(1, VisualizationPolicy.latencyStage(Double.NaN))
        assertEquals(1, VisualizationPolicy.latencyStage(81.0))
    }

    @Test
    fun automaticDetailDownshiftsImmediatelyAndUpshiftsAfterDwell() {
        assertEquals(1, VisualizationPolicy.update(9, 0.2, 16.0, 9, 0))
        assertEquals(5, VisualizationPolicy.update(5, 4.0, 4.0, 9, 1_499))
        assertEquals(6, VisualizationPolicy.update(5, 4.0, 4.0, 9, 1_500))
        assertEquals(7, VisualizationPolicy.update(9, 4.0, 4.0, 7, 2_000))
    }

    @Test
    fun rendererLatencyUsesTheSharedP95Sample() {
        val tracker = VisualizationLatencyTracker()
        (1..20).forEach { tracker.record(it.toDouble()) }
        assertEquals(19.0, tracker.p95Milliseconds(), 1e-9)
        tracker.record(Double.NaN)
        tracker.record(-1.0)
        assertEquals(19.0, tracker.p95Milliseconds(), 1e-9)
    }

    @Test
    fun physicalStagesRequireClearanceAndPublishedSomaAndNeuriteRadii() {
        assertEquals(9, VisualizationPolicy.highestAvailable(null, snapshot(true, 0.04, 0.006, true)))
        assertEquals(6, VisualizationPolicy.highestAvailable(null, snapshot(true, null, 0.006, true)))
        assertEquals(6, VisualizationPolicy.highestAvailable(null, snapshot(false, 0.04, 0.006, true)))
    }

    private fun snapshot(
        clearance: Boolean,
        somaRadius: Double?,
        neuriteRadius: Double,
        includeMarkers: Boolean,
    ) = RemoteDisplaySnapshot(
        schemaVersion = 2,
        morphologyRevision = 1,
        topologyEpoch = 1,
        routeEpoch = 1,
        sequence = 1,
        mode = "anatomical",
        provenance = "procedural_anatomy",
        complete = true,
        truncated = false,
        volumetricClearanceVerified = clearance,
        unavailableReason = null,
        region = null,
        membrane = null,
        nodes = listOf(
            RemoteDisplayNode("1:1", "hidden", 0, RemoteDisplayPoint(0.0, 0.0, 0.0), 0, somaRadius),
            RemoteDisplayNode("2:1", "hidden", 0, RemoteDisplayPoint(1.0, 0.0, 0.0), 1, somaRadius),
        ),
        edges = listOf(
            RemoteDisplayLine("edge", "1:1", "2:1", "axon", emptyList()),
            RemoteDisplayLine("path", "1:1", "1:1", "axon", listOf(RemoteDisplayPoint(0.0, 0.0, 0.0), RemoteDisplayPoint(1.0, 0.0, 0.0)), radius = neuriteRadius, isPath = true),
        ),
        markers = if (includeMarkers) listOf(RemoteDisplayMarker("3:1", "synapse", RemoteDisplayPoint(0.5, 0.0, 0.0))) else emptyList(),
        rawJson = "{}",
    )
}
