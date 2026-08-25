package com.neuralmimicry.aarnn

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.AccountCircle
import androidx.compose.material.icons.filled.Dashboard
import androidx.compose.material.icons.filled.Share
import androidx.compose.material3.Icon
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.withTransform
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent { AarnnRemoteScreen() }
    }
}

private enum class AarnnTab { Dashboard, Graph, Account }

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun AarnnRemoteScreen() {
    val controller = remember { RemoteConnectionController() }
    var endpoint by rememberSaveable { mutableStateOf("http://192.168.1.2") }
    var virtualHost by rememberSaveable { mutableStateOf("aarnn.neuralmimicry.ai") }
    var username by rememberSaveable { mutableStateOf("") }
    var password by remember { mutableStateOf("") }
    var selectedTab by rememberSaveable { mutableIntStateOf(AarnnTab.Dashboard.ordinal) }
    val state = controller.uiState

    DisposableEffect(controller) { onDispose { controller.close() } }

    MaterialTheme {
        Scaffold(
            topBar = {
                TopAppBar(
                    title = {
                        Column {
                            Text("AARNN", fontWeight = FontWeight.Bold)
                            Text(
                                when (selectedTab) {
                                    AarnnTab.Dashboard.ordinal -> "Neural dashboard"
                                    AarnnTab.Graph.ordinal -> "Graph Explorer"
                                    else -> "Account & connection"
                                },
                                style = MaterialTheme.typography.labelMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    },
                    actions = {
                        ConnectionChip(state.state)
                        Spacer(Modifier.width(12.dp))
                    },
                )
            },
            bottomBar = {
                NavigationBar {
                    NavigationBarItem(
                        selected = selectedTab == AarnnTab.Dashboard.ordinal,
                        onClick = { selectedTab = AarnnTab.Dashboard.ordinal },
                        icon = { Icon(Icons.Default.Dashboard, contentDescription = "Dashboard") },
                        label = { Text("Dashboard") },
                    )
                    NavigationBarItem(
                        selected = selectedTab == AarnnTab.Graph.ordinal,
                        onClick = { selectedTab = AarnnTab.Graph.ordinal },
                        icon = { Icon(Icons.Default.Share, contentDescription = "Graph Explorer") },
                        label = { Text("Graph") },
                    )
                    NavigationBarItem(
                        selected = selectedTab == AarnnTab.Account.ordinal,
                        onClick = { selectedTab = AarnnTab.Account.ordinal },
                        icon = { Icon(Icons.Default.AccountCircle, contentDescription = "Account") },
                        label = { Text("Account") },
                    )
                }
            },
        ) { innerPadding ->
            if (selectedTab == AarnnTab.Dashboard.ordinal) {
                DashboardScreen(
                    state = state,
                    contentPadding = innerPadding,
                    onOpenAccount = { selectedTab = AarnnTab.Account.ordinal },
                    onOpenGraph = { selectedTab = AarnnTab.Graph.ordinal },
                    onRefresh = controller::refresh,
                )
            } else if (selectedTab == AarnnTab.Graph.ordinal) {
                GraphExplorerScreen(
                    state = state,
                    contentPadding = innerPadding,
                    onOpenAccount = { selectedTab = AarnnTab.Account.ordinal },
                    onRefresh = controller::refresh,
                )
            } else {
                AccountScreen(
                    state = state,
                    contentPadding = innerPadding,
                    endpoint = endpoint,
                    onEndpointChange = { endpoint = it },
                    virtualHost = virtualHost,
                    onVirtualHostChange = { virtualHost = it },
                    username = username,
                    onUsernameChange = { username = it },
                    password = password,
                    onPasswordChange = { password = it },
                    onConnect = {
                        val submittedPassword = password
                        password = ""
                        controller.connect(endpoint, virtualHost, username, submittedPassword)
                    },
                    onDisconnect = controller::disconnect,
                    onRefresh = controller::refresh,
                )
            }
        }
    }
}

@Composable
private fun DashboardScreen(
    state: RemoteConnectionUiState,
    contentPadding: PaddingValues,
    onOpenAccount: () -> Unit,
    onOpenGraph: () -> Unit,
    onRefresh: () -> Unit,
) {
    val snapshot = state.snapshot
    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(
            start = 16.dp,
            top = contentPadding.calculateTopPadding() + 8.dp,
            end = 16.dp,
            bottom = contentPadding.calculateBottomPadding() + 16.dp,
        ),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        item { DashboardIntro(snapshot, state.state, onOpenAccount) }
        if (snapshot == null) {
            item { EmptyDashboard(state, onOpenAccount) }
        } else {
            item { WorkspaceHero(snapshot, onRefresh) }
            item { MetricsGrid(snapshot) }
            item { NeuralActivityCard(snapshot, onOpenGraph) }
            item { LayerActivityCard(snapshot) }
            item { DistributedNodesCard(snapshot) }
        }
    }
}

@Composable
private fun DashboardIntro(
    snapshot: RemoteWorkspaceSnapshot?,
    connectionState: RemoteConnectionState,
    onOpenAccount: () -> Unit,
) {
    Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.weight(1f)) {
            Text(
                if (snapshot == null) "Observe your brain" else "Live activity",
                style = MaterialTheme.typography.headlineSmall,
                fontWeight = FontWeight.Bold,
            )
            Text(
                if (snapshot == null) "Connect a workspace to begin" else "${snapshot.summary.name.ifBlank { snapshot.summary.networkId }} is streaming",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        if (snapshot == null || connectionState == RemoteConnectionState.Error) {
            OutlinedButton(onClick = onOpenAccount) { Text("Account") }
        }
    }
}

@Composable
private fun EmptyDashboard(state: RemoteConnectionUiState, onOpenAccount: () -> Unit) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = Color(0xFF101A2B)),
    ) {
        Column(
            modifier = Modifier.padding(20.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            EmptyNetworkGraphic()
            Text("No workspace connected", color = Color.White, style = MaterialTheme.typography.titleLarge)
            Text(
                state.error ?: "Use the Account tab to authorise a read-only workspace session.",
                color = Color(0xFFB7C7E5),
            )
            Button(
                onClick = onOpenAccount,
                colors = ButtonDefaults.buttonColors(containerColor = Color(0xFF6D5ACF)),
            ) { Text("Open account") }
        }
    }
}

@Composable
private fun EmptyNetworkGraphic() {
    Canvas(modifier = Modifier.fillMaxWidth().height(110.dp)) {
        val centre = Offset(size.width / 2f, size.height / 2f)
        val points = listOf(
            centre.copy(x = centre.x - 130f, y = centre.y),
            centre.copy(x = centre.x - 45f, y = centre.y - 35f),
            centre.copy(x = centre.x + 45f, y = centre.y + 35f),
            centre.copy(x = centre.x + 130f, y = centre.y),
        )
        for (index in 0 until points.lastIndex) {
            drawLine(Color(0xFF526A9A), points[index], points[index + 1], strokeWidth = 3f)
        }
        points.forEachIndexed { index, point ->
            drawCircle(if (index == 1 || index == 2) Color(0xFF8D7CF0) else Color(0xFF4D9DE0), 12f, point)
            drawCircle(Color.White.copy(alpha = 0.7f), 16f, point, style = Stroke(width = 2f))
        }
    }
}

@Composable
private fun WorkspaceHero(snapshot: RemoteWorkspaceSnapshot, onRefresh: () -> Unit) {
    val summary = snapshot.summary
    Card(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = Color(0xFF101A2B)),
    ) {
        Column(modifier = Modifier.padding(18.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                StatusDot(Color(0xFF52D273))
                Spacer(Modifier.width(8.dp))
                Text(if (summary.running) "RUNNING" else "PAUSED", color = Color(0xFFB4F3C1), fontWeight = FontWeight.Bold)
                Spacer(Modifier.weight(1f))
                TextButton(onClick = onRefresh) { Text("Sync") }
            }
            Text(summary.name.ifBlank { summary.networkId }, color = Color.White, style = MaterialTheme.typography.headlineSmall)
            Text("Owner ${summary.ownerId}  •  ${summary.networkId}", color = Color(0xFFB7C7E5))
            Row(horizontalArrangement = Arrangement.spacedBy(24.dp)) {
                HeroValue("STEP", snapshot.activity.step.toString())
                HeroValue("LOGICAL TIME", formatMilliseconds(snapshot.activity.simTimeMs))
            }
            Text("Read-only observation  •  refresh does not drive neural sampling", color = Color(0xFF8294B7), style = MaterialTheme.typography.labelSmall)
        }
    }
}

@Composable
private fun HeroValue(label: String, value: String) {
    Column {
        Text(label, color = Color(0xFF8294B7), style = MaterialTheme.typography.labelSmall)
        Text(value, color = Color.White, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
    }
}

@Composable
private fun MetricsGrid(snapshot: RemoteWorkspaceSnapshot) {
    val summary = snapshot.summary
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp), modifier = Modifier.fillMaxWidth()) {
            MetricCard("Neurons", summary.totalNeurons.toString(), "◉", Color(0xFF5C9DFF), Modifier.weight(1f))
            MetricCard("Layers", summary.hiddenLayers.toString(), "≋", Color(0xFFB18CFF), Modifier.weight(1f))
        }
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp), modifier = Modifier.fillMaxWidth()) {
            MetricCard("Active spikes", activeSpikeCount(snapshot).toString(), "✦", Color(0xFFFFC857), Modifier.weight(1f))
            MetricCard("Nodes", summary.distributedNodeIds.size.toString(), "◆", Color(0xFF54D6A0), Modifier.weight(1f))
        }
    }
}

@Composable
private fun MetricCard(label: String, value: String, glyph: String, accent: Color, modifier: Modifier) {
    Card(modifier = modifier, colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant)) {
        Column(modifier = Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Text(glyph, color = accent, style = MaterialTheme.typography.titleLarge)
            Text(value, style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.Bold)
            Text(label, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.labelMedium)
        }
    }
}

@Composable
private fun NeuralActivityCard(snapshot: RemoteWorkspaceSnapshot, onOpenGraph: () -> Unit) {
    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(modifier = Modifier.weight(1f)) {
                    Text("Neural topology", style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold)
                    Text("Live active-neuron projection", color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodySmall)
                }
                TextButton(onClick = onOpenGraph) { Text("Explore") }
            }
            NeuralNetworkCanvas(snapshot)
        }
    }
}

@Composable
private fun GraphExplorerScreen(
    state: RemoteConnectionUiState,
    contentPadding: PaddingValues,
    onOpenAccount: () -> Unit,
    onRefresh: () -> Unit,
) {
    var zoom by rememberSaveable { mutableFloatStateOf(1f) }
    var rotation by rememberSaveable { mutableFloatStateOf(0f) }
    var panX by rememberSaveable { mutableFloatStateOf(0f) }
    var panY by rememberSaveable { mutableFloatStateOf(0f) }
    val snapshot = state.snapshot

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(
                start = 12.dp,
                top = contentPadding.calculateTopPadding() + 8.dp,
                end = 12.dp,
                bottom = contentPadding.calculateBottomPadding() + 10.dp,
            ),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth()) {
            Column(modifier = Modifier.weight(1f)) {
                Text("Graph Explorer", style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.Bold)
                Text(
                    snapshot?.let { it.summary.name.ifBlank { it.summary.networkId } }
                        ?: "Connect to inspect the neural topology",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodySmall,
                )
                if (snapshot != null) {
                    Text(
                        if (snapshot.topology.edges.isNotEmpty()) {
                            "Authoritative topology • ${snapshot.topology.edges.size} visible edges${if (snapshot.topology.truncated) " • bounded" else ""}"
                        } else {
                            "Topology projection unavailable"
                        },
                        color = if (snapshot.topology.edges.isNotEmpty()) Color(0xFF1B7A45) else MaterialTheme.colorScheme.error,
                        style = MaterialTheme.typography.labelSmall,
                    )
                }
            }
            if (snapshot != null) {
                TextButton(onClick = onRefresh) { Text("Sync") }
            } else {
                OutlinedButton(onClick = onOpenAccount) { Text("Connect") }
            }
        }

        GraphExplorerCanvas(
            snapshot = snapshot,
            zoom = zoom,
            rotation = rotation,
            pan = Offset(panX, panY),
            onTransform = { gestureZoom, gestureRotation, gesturePan ->
                zoom = (zoom * gestureZoom).coerceIn(MIN_GRAPH_ZOOM, MAX_GRAPH_ZOOM)
                rotation = (rotation + gestureRotation).coerceIn(-180f, 180f)
                panX += gesturePan.x
                panY += gesturePan.y
            },
            modifier = Modifier
                .fillMaxWidth()
                .weight(1f),
        )

        GraphControls(
            zoom = zoom,
            rotation = rotation,
            onZoomChange = { zoom = it },
            onRotationChange = { rotation = it },
            onReset = {
                zoom = 1f
                rotation = 0f
                panX = 0f
                panY = 0f
            },
        )
        GraphLegend()
    }
}

@Composable
private fun GraphExplorerCanvas(
    snapshot: RemoteWorkspaceSnapshot?,
    zoom: Float,
    rotation: Float,
    pan: Offset,
    onTransform: (Float, Float, Offset) -> Unit,
    modifier: Modifier,
) {
    val layers = graphLayers(snapshot)
    val topologyEdges = snapshot?.topology?.edges.orEmpty()
    val nodeIds = layers.flatMapIndexed { column, layer ->
        layer.visibleNodeIds.mapIndexed { node, id -> id to nodePointKey(column, node) }
    }.toMap()
    Canvas(
        modifier = modifier
            .clip(RoundedCornerShape(18.dp))
            .background(Color(0xFF0B1018))
            .pointerInput(Unit) {
                detectTransformGestures { _, gesturePan, gestureZoom, gestureRotation ->
                    onTransform(gestureZoom, gestureRotation, gesturePan)
                }
            },
    ) {
        drawRect(Color(0xFF0B1018))
        val centre = Offset(size.width / 2f, size.height / 2f)
        val positions = graphNodePositions(layers, size.width, size.height)

        withTransform({
            translate(left = pan.x, top = pan.y)
            rotate(degrees = rotation, pivot = centre)
            scale(scaleX = zoom, scaleY = zoom, pivot = centre)
        }) {
            for (line in 1..5) {
                val y = size.height * line / 6f
                drawLine(Color(0xFF233044).copy(alpha = 0.45f), Offset(0f, y), Offset(size.width, y), 1f)
            }
            if (topologyEdges.isNotEmpty()) {
                topologyEdges.forEach { edge ->
                    val start = nodeIds[edge.sourceId]?.let { key -> positions[key.first].getOrNull(key.second) }
                    val target = nodeIds[edge.targetId]?.let { key -> positions[key.first].getOrNull(key.second) }
                    if (start != null && target != null) {
                        val colour = if (edge.weight < 0.0) Color(0xFF65C7FF) else Color(0xFFFF8A00)
                        val alpha = (0.12f + edge.weight.toFloat().coerceIn(-1f, 1f).let { kotlin.math.abs(it) } * 0.25f)
                            .coerceIn(0.12f, 0.38f)
                        drawLine(colour.copy(alpha = alpha), start, target, strokeWidth = 1.1f)
                    }
                }
            } else if (snapshot == null) {
                // The disconnected screen is an explicitly labelled local
                // demonstration. Never invent connection lines for a live
                // workspace whose authoritative topology is unavailable.
                for (column in 0 until positions.lastIndex) {
                    val source = positions[column]
                    val target = positions[column + 1]
                    if (source.isNotEmpty() && target.isNotEmpty()) {
                        val fanOut = (MAX_GRAPH_EDGES / source.size.coerceAtLeast(1)).coerceIn(2, target.size)
                        source.forEachIndexed { sourceIndex, start ->
                            repeat(fanOut) { edgeIndex ->
                                val targetIndex = (sourceIndex * 13 + edgeIndex * 7 + column * 5) % target.size
                                drawLine(
                                    Color(0xFFFF8A00).copy(alpha = if (edgeIndex == 0) 0.34f else 0.16f),
                                    start,
                                    target[targetIndex],
                                    strokeWidth = if (edgeIndex == 0) 1.7f else 0.9f,
                                )
                            }
                        }
                    }
                }
            }
            positions.forEachIndexed { column, nodes ->
                val layer = layers[column]
                val colour = graphLayerColour(column, layers.lastIndex)
                nodes.forEachIndexed { node, point ->
                    val active = layer.activeIndices.any { index ->
                        if (layer.count > layer.visibleNodeIds.size) {
                            index * layer.visibleNodeIds.size / layer.count == node
                        } else {
                            index == node
                        }
                    }
                    if (active) {
                        drawCircle(Color(0xFFFFB84A).copy(alpha = 0.22f), 11f, point)
                        drawCircle(Color.White.copy(alpha = 0.9f), 6.3f, point, style = Stroke(width = 1.3f))
                    }
                    drawCircle(colour.copy(alpha = if (active) 1f else 0.62f), if (active) 4.7f else 3.5f, point)
                }
                drawLine(
                    colour.copy(alpha = 0.7f),
                    Offset(nodes.firstOrNull()?.x ?: 0f, 16f),
                    Offset(nodes.firstOrNull()?.x ?: 0f, size.height - 16f),
                    strokeWidth = 1f,
                )
            }
        }

        if (snapshot == null) {
            drawCircle(Color(0xFF6D5ACF).copy(alpha = 0.18f), 58f, centre)
        }
    }
}

@Composable
private fun GraphControls(
    zoom: Float,
    rotation: Float,
    onZoomChange: (Float) -> Unit,
    onRotationChange: (Float) -> Unit,
    onReset: () -> Unit,
) {
    Card(modifier = Modifier.fillMaxWidth(), colors = CardDefaults.cardColors(containerColor = Color(0xFF121D2D))) {
        Column(modifier = Modifier.padding(horizontal = 12.dp, vertical = 7.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            GraphSliderRow("Zoom", zoom, MIN_GRAPH_ZOOM, MAX_GRAPH_ZOOM, "%.1fx", onZoomChange)
            GraphSliderRow("Rotate", rotation, -180f, 180f, "%.0f°", onRotationChange)
            Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth()) {
                Text("Two-finger drag to pan • pinch to zoom", color = Color(0xFF9AAAC5), style = MaterialTheme.typography.labelSmall, modifier = Modifier.weight(1f))
                TextButton(onClick = onReset) { Text("Reset camera") }
            }
        }
    }
}

@Composable
private fun GraphSliderRow(
    label: String,
    value: Float,
    minimum: Float,
    maximum: Float,
    format: String,
    onValueChange: (Float) -> Unit,
) {
    Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth()) {
        Text(label, color = Color(0xFFEAF0FF), modifier = Modifier.width(56.dp), style = MaterialTheme.typography.labelMedium)
        androidx.compose.material3.Slider(
            value = value,
            onValueChange = onValueChange,
            valueRange = minimum..maximum,
            modifier = Modifier.weight(1f),
        )
        Text(
            String.format(Locale.US, format, value),
            color = Color(0xFFB7C7E5),
            modifier = Modifier.width(48.dp),
            style = MaterialTheme.typography.labelSmall,
        )
    }
}

@Composable
private fun GraphLegend() {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceEvenly,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        GraphLegendItem("Sensory", Color(0xFF4D9DE0))
        GraphLegendItem("Hidden", Color(0xFF9B7CFF))
        GraphLegendItem("Output", Color(0xFFFFC857))
        GraphLegendItem("Active", Color(0xFFFFB84A))
    }
}

@Composable
private fun GraphLegendItem(label: String, colour: Color) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        StatusDot(colour, 7.dp)
        Spacer(Modifier.width(4.dp))
        Text(label, style = MaterialTheme.typography.labelSmall)
    }
}

private data class GraphLayer(
    val id: String,
    val count: Int,
    val activeIndices: List<Int>,
    val visibleNodeIds: List<String>,
)

private fun graphLayers(snapshot: RemoteWorkspaceSnapshot?): List<GraphLayer> {
    if (snapshot == null) {
        return listOf(
            demoGraphLayer("sensory", 8),
            demoGraphLayer("hidden-0", 16),
            demoGraphLayer("hidden-1", 16),
            demoGraphLayer("output", 8),
        )
    }
    val topology = snapshot.topology
    if (topology.layers.isNotEmpty() && topology.nodes.isNotEmpty()) {
        return topology.layers.map { layer ->
            val nodes = topology.nodes
                .asSequence()
                .filter { it.layerId == layer.id }
                .sortedBy { it.index }
                .toList()
            GraphLayer(
                id = layer.id,
                count = layer.neuronCount,
                activeIndices = nodes.mapIndexedNotNull { index, node -> node.active.takeIf { it }?.let { index } },
                visibleNodeIds = nodes.map { it.id },
            )
        }.takeIf { it.isNotEmpty() } ?: emptyList()
    }
    val summary = snapshot.summary
    val hiddenTotal = (summary.totalNeurons - summary.sensoryNeurons - summary.outputNeurons).coerceAtLeast(0)
    val hiddenCounts = List(summary.hiddenLayers.coerceAtLeast(0)) { index ->
        val base = if (summary.hiddenLayers == 0) 0 else hiddenTotal / summary.hiddenLayers
        if (index == summary.hiddenLayers - 1) hiddenTotal - base * (summary.hiddenLayers - 1) else base
    }
    return listOf(demoGraphLayer("sensory", summary.sensoryNeurons, snapshot.activity.sensory)) +
        hiddenCounts.mapIndexed { index, count ->
            demoGraphLayer("hidden-$index", count, snapshot.activity.hidden.getOrNull(index).orEmpty())
        } +
        listOf(demoGraphLayer("output", summary.outputNeurons, snapshot.activity.output))
}

private fun demoGraphLayer(id: String, count: Int, activeIndices: List<Int> = emptyList()): GraphLayer {
    val visible = count.coerceIn(1, MAX_GRAPH_NODES)
    val ids = List(visible) { index -> "$id:$index" }
    return GraphLayer(id, count, activeIndices, ids)
}

private fun nodePointKey(column: Int, node: Int): Pair<Int, Int> = column to node

private fun graphNodePositions(layers: List<GraphLayer>, width: Float, height: Float): List<List<Offset>> {
    val columnWidth = width / (layers.size + 1).coerceAtLeast(2)
    return layers.mapIndexed { column, layer ->
        val visible = layer.visibleNodeIds.size.coerceIn(1, MAX_GRAPH_NODES)
        val x = columnWidth * (column + 1)
        val spacing = (height - 44f) / visible.coerceAtLeast(1)
        (0 until visible).map { node -> Offset(x, 22f + spacing * (node + 0.5f)) }
    }
}

private fun graphLayerColour(column: Int, lastColumn: Int): Color = when {
    column == 0 -> Color(0xFF4D9DE0)
    column == lastColumn -> Color(0xFFFFC857)
    else -> Color(0xFF9B7CFF)
}

@Composable
private fun NeuralNetworkCanvas(snapshot: RemoteWorkspaceSnapshot) {
    val summary = snapshot.summary
    val hiddenTotal = (summary.totalNeurons - summary.sensoryNeurons - summary.outputNeurons).coerceAtLeast(0)
    val hiddenCounts = List(summary.hiddenLayers.coerceAtLeast(0)) { index ->
        val base = if (summary.hiddenLayers == 0) 0 else hiddenTotal / summary.hiddenLayers
        if (index == summary.hiddenLayers - 1) hiddenTotal - base * (summary.hiddenLayers - 1) else base
    }
    val counts = listOf(summary.sensoryNeurons) + hiddenCounts + listOf(summary.outputNeurons)
    val active = listOf(snapshot.activity.sensory) + snapshot.activity.hidden + listOf(snapshot.activity.output)
    val labels = listOf("Sensory") + List(hiddenCounts.size) { "Hidden ${it + 1}" } + listOf("Output")

    Canvas(
        modifier = Modifier
            .fillMaxWidth()
            .height(220.dp)
            .clip(RoundedCornerShape(16.dp))
            .background(Color(0xFF0B1322)),
    ) {
        val columnWidth = size.width / counts.size.coerceAtLeast(1)
        val centreY = size.height / 2f
        val visibleCounts = counts.map { it.coerceIn(0, MAX_VISIBLE_NODES) }
        val positions = visibleCounts.mapIndexed { column, visible ->
            val x = columnWidth * (column + 0.5f)
            (0 until visible).map { node ->
                Offset(x, centreY + (node - (visible - 1) / 2f) * NODE_SPACING)
            }
        }

        for (column in 0 until positions.lastIndex) {
            val source = positions[column]
            val target = positions[column + 1]
            if (target.isNotEmpty()) {
                source.forEachIndexed { index, start ->
                    val end = target[(index * target.size / source.size).coerceAtMost(target.lastIndex)]
                    drawLine(Color(0xFF53637F).copy(alpha = 0.28f), start, end, strokeWidth = 1.5f)
                }
            }
        }
        positions.forEachIndexed { column, nodes ->
            val colour = when {
                column == 0 -> Color(0xFF4D9DE0)
                column == positions.lastIndex -> Color(0xFFFFC857)
                else -> Color(0xFF9B7CFF)
            }
            nodes.forEachIndexed { node, offset ->
                val activeIndices = active.getOrNull(column).orEmpty()
                val isActive = activeIndices.any { index ->
                    if (counts[column] > MAX_VISIBLE_NODES) index * MAX_VISIBLE_NODES / counts[column] == node else index == node
                }
                drawCircle(if (isActive) colour else colour.copy(alpha = 0.2f), NODE_RADIUS + if (isActive) 1.5f else 0f, offset)
                if (isActive) drawCircle(Color.White.copy(alpha = 0.9f), NODE_RADIUS + 2.5f, offset, style = Stroke(width = 1.2f))
            }
        }
    }
    Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceEvenly) {
        labels.forEachIndexed { index, label ->
            Row(verticalAlignment = Alignment.CenterVertically) {
                StatusDot(if (index == 0) Color(0xFF4D9DE0) else if (index == labels.lastIndex) Color(0xFFFFC857) else Color(0xFF9B7CFF), size = 7.dp)
                Spacer(Modifier.width(4.dp))
                Text(label, style = MaterialTheme.typography.labelSmall)
            }
        }
    }
}

@Composable
private fun LayerActivityCard(snapshot: RemoteWorkspaceSnapshot) {
    val summary = snapshot.summary
    val hiddenTotal = (summary.totalNeurons - summary.sensoryNeurons - summary.outputNeurons).coerceAtLeast(0)
    val hiddenCounts = List(summary.hiddenLayers.coerceAtLeast(0)) { index ->
        val base = if (summary.hiddenLayers == 0) 0 else hiddenTotal / summary.hiddenLayers
        if (index == summary.hiddenLayers - 1) hiddenTotal - base * (summary.hiddenLayers - 1) else base
    }
    val rows = listOf("Sensory" to summary.sensoryNeurons) +
        hiddenCounts.mapIndexed { index, count -> "Hidden ${index + 1}" to count } +
        listOf("Output" to summary.outputNeurons)
    val activity = listOf(snapshot.activity.sensory) + snapshot.activity.hidden + listOf(snapshot.activity.output)

    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text("Activity by layer", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
            rows.forEachIndexed { index, (name, count) ->
                ActivityBar(name, activity.getOrNull(index)?.size ?: 0, count)
            }
        }
    }
}

@Composable
private fun ActivityBar(label: String, active: Int, total: Int) {
    val fraction = if (total > 0) (active.toFloat() / total).coerceIn(0f, 1f) else 0f
    Column(verticalArrangement = Arrangement.spacedBy(5.dp)) {
        Row {
            Text(label, style = MaterialTheme.typography.labelMedium, modifier = Modifier.weight(1f))
            Text("$active / $total", color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.labelSmall)
        }
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .height(8.dp)
                .clip(CircleShape)
                .background(MaterialTheme.colorScheme.surfaceVariant),
        ) {
            Box(
                modifier = Modifier
                    .fillMaxWidth(fraction)
                    .height(8.dp)
                    .clip(CircleShape)
                    .background(Color(0xFF8069D7)),
            )
        }
    }
}

@Composable
private fun DistributedNodesCard(snapshot: RemoteWorkspaceSnapshot) {
    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("Distributed nodes", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
                Spacer(Modifier.weight(1f))
                Text("${snapshot.summary.distributedNodeIds.size} ONLINE", color = Color(0xFF1B7A45), style = MaterialTheme.typography.labelSmall, fontWeight = FontWeight.Bold)
            }
            if (snapshot.summary.distributedNodeIds.isEmpty()) {
                Text("Node details are not exposed by the current workspace projection", color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodySmall)
            } else {
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.fillMaxWidth()) {
                    snapshot.summary.distributedNodeIds.take(4).forEach { node -> NodeBadge(node) }
                }
                if (snapshot.summary.distributedNodeIds.size > 4) {
                    Text("+${snapshot.summary.distributedNodeIds.size - 4} more nodes", color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.labelSmall)
                }
            }
        }
    }
}

@Composable
private fun RowScope.NodeBadge(node: String) {
    Surface(
        modifier = Modifier.weight(1f),
        shape = RoundedCornerShape(12.dp),
        color = MaterialTheme.colorScheme.surfaceVariant,
    ) {
        Column(modifier = Modifier.padding(10.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                StatusDot(Color(0xFF52D273), 7.dp)
                Spacer(Modifier.width(5.dp))
                Text(node.take(10), style = MaterialTheme.typography.labelSmall, maxLines = 1)
            }
            Text("ready", color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.labelSmall)
        }
    }
}

@Composable
private fun AccountScreen(
    state: RemoteConnectionUiState,
    contentPadding: PaddingValues,
    endpoint: String,
    onEndpointChange: (String) -> Unit,
    virtualHost: String,
    onVirtualHostChange: (String) -> Unit,
    username: String,
    onUsernameChange: (String) -> Unit,
    password: String,
    onPasswordChange: (String) -> Unit,
    onConnect: () -> Unit,
    onDisconnect: () -> Unit,
    onRefresh: () -> Unit,
) {
    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(
            start = 16.dp,
            top = contentPadding.calculateTopPadding() + 8.dp,
            end = 16.dp,
            bottom = contentPadding.calculateBottomPadding() + 16.dp,
        ),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        item { AccountHeader(username, state.state) }
        item {
            ConnectionForm(
                endpoint = endpoint,
                onEndpointChange = onEndpointChange,
                virtualHost = virtualHost,
                onVirtualHostChange = onVirtualHostChange,
                username = username,
                onUsernameChange = onUsernameChange,
                password = password,
                onPasswordChange = onPasswordChange,
                state = state.state,
                onConnect = onConnect,
                onDisconnect = onDisconnect,
            )
        }
        item { SessionCard(state, onRefresh) }
        item { CapabilityCard() }
        item { SecurityCard() }
    }
}

@Composable
private fun AccountHeader(username: String, state: RemoteConnectionState) {
    Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth()) {
        Box(
            modifier = Modifier.size(58.dp).clip(CircleShape).background(Color(0xFF6D5ACF)),
            contentAlignment = Alignment.Center,
        ) { Text(if (username.isBlank()) "A" else username.take(1).uppercase(), color = Color.White, style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.Bold) }
        Spacer(Modifier.width(14.dp))
        Column(modifier = Modifier.weight(1f)) {
            Text(if (username.isBlank()) "Guest account" else username, style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold)
            Text(if (state == RemoteConnectionState.Connected) "Workspace session active" else "Read-only workspace access", color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        ConnectionChip(state)
    }
}

@Composable
private fun ConnectionForm(
    endpoint: String,
    onEndpointChange: (String) -> Unit,
    virtualHost: String,
    onVirtualHostChange: (String) -> Unit,
    username: String,
    onUsernameChange: (String) -> Unit,
    password: String,
    onPasswordChange: (String) -> Unit,
    state: RemoteConnectionState,
    onConnect: () -> Unit,
    onDisconnect: () -> Unit,
) {
    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Text("Workspace access", style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold)
            Text("Authorise a read-only session through the Rust gateway.", color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodySmall)
            OutlinedTextField(endpoint, onEndpointChange, label = { Text("Gateway URL") }, singleLine = true, modifier = Modifier.fillMaxWidth())
            OutlinedTextField(virtualHost, onVirtualHostChange, label = { Text("Ingress host") }, singleLine = true, modifier = Modifier.fillMaxWidth())
            OutlinedTextField(username, onUsernameChange, label = { Text("Username") }, singleLine = true, modifier = Modifier.fillMaxWidth())
            OutlinedTextField(
                password,
                onPasswordChange,
                label = { Text("Password") },
                supportingText = { Text("Cleared immediately after submit; never persisted") },
                singleLine = true,
                visualTransformation = PasswordVisualTransformation(),
                modifier = Modifier.fillMaxWidth(),
            )
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp), modifier = Modifier.fillMaxWidth()) {
                Button(onClick = onConnect, enabled = state != RemoteConnectionState.Connecting, modifier = Modifier.weight(1f)) { Text(if (state == RemoteConnectionState.Connected) "Reconnect" else "Connect") }
                OutlinedButton(onClick = onDisconnect, enabled = state != RemoteConnectionState.Idle, modifier = Modifier.weight(1f)) { Text("Disconnect") }
            }
            Text("HTTP is for the development emulator only. Release builds require HTTPS and server identity validation.", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

@Composable
private fun SessionCard(state: RemoteConnectionUiState, onRefresh: () -> Unit) {
    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Text("Session", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
            StatusRow("Authentication", when (state.state) {
                RemoteConnectionState.Connected -> "Authorised"
                RemoteConnectionState.Connecting -> "In progress"
                RemoteConnectionState.Error -> "Failed"
                RemoteConnectionState.Idle -> "Not connected"
            }, state.state == RemoteConnectionState.Connected)
            state.error?.let { Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall) }
            state.lastUpdatedMs?.let { Text("Last sync ${formatClock(it)}", color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.labelSmall) }
            if (state.state == RemoteConnectionState.Connected) {
                OutlinedButton(onClick = onRefresh, modifier = Modifier.fillMaxWidth()) { Text("Refresh workspace") }
            }
        }
    }
}

@Composable
private fun CapabilityCard() {
    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(9.dp)) {
            Text("Device capabilities", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
            CapabilityRow("Rust local runtime", NativeBridge.isAvailable(), "Reference ABI available")
            CapabilityRow("USB AER input/output", false, "Separately authorised")
            CapabilityRow("Media and federation", false, "Production adapters gated")
            CapabilityRow("Global HID", false, "Unavailable by default")
        }
    }
}

@Composable
private fun CapabilityRow(label: String, available: Boolean, detail: String) {
    Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth()) {
        StatusDot(if (available) Color(0xFF52D273) else Color(0xFFE5A33A))
        Spacer(Modifier.width(10.dp))
        Column(modifier = Modifier.weight(1f)) {
            Text(label, style = MaterialTheme.typography.bodyMedium)
            Text(detail, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.labelSmall)
        }
        Text(if (available) "READY" else "GATED", color = if (available) Color(0xFF1B7A45) else Color(0xFF9A6413), style = MaterialTheme.typography.labelSmall, fontWeight = FontWeight.Bold)
    }
}

@Composable
private fun SecurityCard() {
    Card(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = Color(0xFFFFF7E6)),
    ) {
        Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Text("Privacy & safety", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
            Text("This Android shell observes workspace projections only. It never connects directly to workers, stores passwords, or enables AER/media/HID capabilities from discovery.", style = MaterialTheme.typography.bodySmall)
        }
    }
}

@Composable
private fun ConnectionChip(state: RemoteConnectionState) {
    val (label, colour) = when (state) {
        RemoteConnectionState.Connected -> "LIVE" to Color(0xFF1B7A45)
        RemoteConnectionState.Connecting -> "SYNC" to Color(0xFF9A6413)
        RemoteConnectionState.Error -> "ERROR" to MaterialTheme.colorScheme.error
        RemoteConnectionState.Idle -> "OFFLINE" to MaterialTheme.colorScheme.onSurfaceVariant
    }
    Surface(shape = RoundedCornerShape(50), color = colour.copy(alpha = 0.12f)) {
        Row(modifier = Modifier.padding(horizontal = 10.dp, vertical = 6.dp), verticalAlignment = Alignment.CenterVertically) {
            StatusDot(colour, 7.dp)
            Spacer(Modifier.width(5.dp))
            Text(label, color = colour, style = MaterialTheme.typography.labelSmall, fontWeight = FontWeight.Bold)
        }
    }
}

@Composable
private fun StatusRow(label: String, value: String, positive: Boolean) {
    Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth()) {
        StatusDot(if (positive) Color(0xFF52D273) else Color(0xFFE5A33A))
        Spacer(Modifier.width(10.dp))
        Text(label, modifier = Modifier.weight(1f))
        Text(value, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.labelMedium)
    }
}

@Composable
private fun StatusDot(color: Color, size: Dp = 9.dp) {
    Box(modifier = Modifier.size(size).clip(CircleShape).background(color))
}

private fun activeSpikeCount(snapshot: RemoteWorkspaceSnapshot): Int =
    snapshot.activity.sensory.size + snapshot.activity.hidden.sumOf { it.size } + snapshot.activity.output.size

private fun formatMilliseconds(value: Double): String = String.format(Locale.US, "%.1f ms", value)

private fun formatClock(value: Long): String = SimpleDateFormat("HH:mm:ss", Locale.UK).format(Date(value))

private const val MAX_VISIBLE_NODES = 48
private const val NODE_SPACING = 4.5f
private const val NODE_RADIUS = 3.2f
private const val MIN_GRAPH_ZOOM = 0.65f
private const val MAX_GRAPH_ZOOM = 2.75f
private const val MAX_GRAPH_NODES = 30
private const val MAX_GRAPH_EDGES = 220
