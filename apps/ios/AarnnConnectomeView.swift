import SwiftUI

/// Read-only connectome projection with the shared nine-stage detail control.
/// All geometry comes from an immutable display snapshot; it never accesses
/// neural traversal state or requests a simulation pause.
public struct AarnnConnectomeView: View {
    public let views: AarnnRemoteSession.DisplayViews
    public let activeNodeIDs: Set<AarnnRemoteSession.DisplayID>
    public let activity: AarnnRemoteSession.Activity?
    public let sensoryCount: Int
    public let outputCount: Int

    @State private var stage = 5
    @State private var automatic = true
    @State private var zoom = 1.0
    @State private var zoomAtGestureStart: Double?
    @State private var stageChangedAt = ProcessInfo.processInfo.systemUptime
    @State private var latency = VisualizationLatencyTracker()

    public init(
        views: AarnnRemoteSession.DisplayViews,
        activeNodeIDs: Set<AarnnRemoteSession.DisplayID> = [],
        activity: AarnnRemoteSession.Activity? = nil,
        sensoryCount: Int = 0,
        outputCount: Int = 0
    ) {
        self.views = views
        self.activeNodeIDs = activeNodeIDs
        self.activity = activity
        self.sensoryCount = sensoryCount
        self.outputCount = outputCount
    }

    private var highestAvailableStage: Int {
        AarnnVisualizationPolicy.highestAvailable(
            synthetic: views.syntheticColumns,
            anatomical: views.anatomical
        )
    }

    private var resolvedStage: Int {
        min(max(stage, 1), highestAvailableStage)
    }

    private var selectedSnapshot: AarnnRemoteSession.DisplaySnapshot? {
        views.snapshot(for: resolvedStage <= 3 ? .syntheticColumns : .anatomical)
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            VStack(alignment: .leading, spacing: 4) {
                HStack {
                    Text("Visual detail")
                        .font(.headline)
                    Spacer()
                    Toggle("Auto", isOn: Binding(
                        get: { automatic },
                        set: { automatic = $0; stageChangedAt = ProcessInfo.processInfo.systemUptime }
                    ))
                    .labelsHidden()
                    Text("Auto")
                        .font(.subheadline)
                }
                HStack(spacing: 8) {
                    Text("Simplest").font(.caption2)
                    Slider(value: Binding(
                        get: { Double(stage) },
                        set: { value in
                            stage = Int(value.rounded()).clamped(to: 1...9)
                            automatic = false
                            stageChangedAt = ProcessInfo.processInfo.systemUptime
                        }
                    ), in: 1...9, step: 1)
                    .accessibilityLabel("Visualisation complexity")
                    Text("Most detailed").font(.caption2)
                }
                let stageName = AarnnVisualizationPolicy.stages[resolvedStage - 1]
                Text("\(automatic ? "Auto" : "Manual") · stage \(resolvedStage) · \(stageName)" +
                     (resolvedStage < stage ? " · geometry available through stage \(highestAvailableStage)" : ""))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            HStack(spacing: 8) {
                Text("Zoom").font(.caption)
                Slider(value: Binding(
                    get: { zoom },
                    set: { setZoom($0) }
                ), in: 0.2...4)
                Text(String(format: "%.1f×", zoom)).font(.caption.monospacedDigit())
            }

            if let snapshot = selectedSnapshot {
                Canvas { context, size in
                    let started = ProcessInfo.processInfo.systemUptime
                    draw(snapshot: snapshot, stage: resolvedStage, zoom: zoom, in: &context, size: size)
                    latency.record(milliseconds: max(0, ProcessInfo.processInfo.systemUptime - started) * 1_000)
                }
                .frame(minHeight: 300)
                .background(Color.black.opacity(0.9))
                .clipShape(RoundedRectangle(cornerRadius: 12))
                .gesture(MagnificationGesture().onChanged { factor in
                    let initialZoom = zoomAtGestureStart ?? zoom
                    zoomAtGestureStart = initialZoom
                    setZoom(initialZoom * factor)
                }.onEnded { _ in
                    zoomAtGestureStart = nil
                })
                Text("\(snapshot.provenance) • sequence \(snapshot.sequence)\(snapshot.truncated ? " • bounded" : "")")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            } else {
                ContentUnavailableView("Display unavailable", systemImage: "point.3.connected.trianglepath.dotted")
            }
            if let activity {
                AarnnSpikeRastersView(activity: activity, sensoryCount: sensoryCount, outputCount: outputCount)
            }
        }
        .padding()
        .task(id: "\(automatic)-\(zoom)-\(highestAvailableStage)") {
            guard automatic else { return }
            while !Task.isCancelled {
                do {
                    try await Task.sleep(nanoseconds: 250_000_000)
                } catch {
                    return
                }
                guard automatic else { return }
                let now = ProcessInfo.processInfo.systemUptime
                let next = AarnnVisualizationPolicy.update(
                    current: stage,
                    zoom: zoom,
                    latencyMilliseconds: latency.p95Milliseconds(),
                    highestAvailable: highestAvailableStage,
                    dwellMilliseconds: UInt64(max(0, now - stageChangedAt) * 1_000)
                )
                if next != stage {
                    stage = next
                    stageChangedAt = now
                }
            }
        }
    }

    /// Keep gesture values relative to the gesture's initial zoom and restart
    /// Auto's dwell whenever the view scale changes.
    private func setZoom(_ value: Double) {
        zoom = min(4, max(0.2, value.isFinite ? value : 1))
        stageChangedAt = ProcessInfo.processInfo.systemUptime
    }

    private func draw(
        snapshot: AarnnRemoteSession.DisplaySnapshot,
        stage: Int,
        zoom: Double,
        in context: inout GraphicsContext,
        size: CGSize
    ) {
        let allPoints = snapshot.nodes.map(\.positionMM)
            + snapshot.edges.flatMap(\.pointsMM)
            + snapshot.paths.flatMap(\.pointsMM)
            + snapshot.markers.map(\.positionMM)
        let minX = snapshot.region?.min.x ?? allPoints.map(\.x).min() ?? -1
        let maxX = snapshot.region?.max.x ?? allPoints.map(\.x).max() ?? 1
        let minY = snapshot.region?.min.y ?? allPoints.map(\.y).min() ?? -1
        let maxY = snapshot.region?.max.y ?? allPoints.map(\.y).max() ?? 1
        let spanX = max(maxX - minX, 0.000001)
        let spanY = max(maxY - minY, 0.000001)
        let fitScale = min(max(1, size.width - 28) / CGFloat(spanX), max(1, size.height - 28) / CGFloat(spanY))
        let physicalScale = fitScale * CGFloat(zoom)

        func point(_ value: AarnnRemoteSession.DisplayPoint) -> CGPoint {
            CGPoint(
                x: size.width / 2 + CGFloat(value.x - (minX + maxX) / 2) * physicalScale,
                y: size.height / 2 - CGFloat(value.y - (minY + maxY) / 2) * physicalScale
            )
        }

        var scene = context
        scene.fill(Path(CGRect(origin: .zero, size: size)), with: .color(.black.opacity(0.9)))
        if snapshot.mode == .anatomical, let membrane = snapshot.membrane {
            let minimum = point(.init(
                x: membrane.centreMM.x - membrane.radiiMM.x,
                y: membrane.centreMM.y - membrane.radiiMM.y,
                z: membrane.centreMM.z - membrane.radiiMM.z
            ))
            let maximum = point(.init(
                x: membrane.centreMM.x + membrane.radiiMM.x,
                y: membrane.centreMM.y + membrane.radiiMM.y,
                z: membrane.centreMM.z + membrane.radiiMM.z
            ))
            let membranePath = Path(ellipseIn: CGRect(
                x: min(minimum.x, maximum.x),
                y: min(minimum.y, maximum.y),
                width: abs(maximum.x - minimum.x),
                height: abs(maximum.y - minimum.y)
            ))
            scene.fill(membranePath, with: .color(Color(red: 0.47, green: 0.57, blue: 0.75).opacity(0.10)))
            scene.clip(to: membranePath)
        }

        var nodeByID: [AarnnRemoteSession.DisplayID: AarnnRemoteSession.DisplayNode] = [:]
        var screenByID: [AarnnRemoteSession.DisplayID: CGPoint] = [:]
        for node in snapshot.nodes {
            nodeByID[node.id] = node
            screenByID[node.id] = point(node.positionMM)
        }

        if stage == 2 || stage == 3 || stage == 5 || stage == 6 {
            var usedSources = Set<AarnnRemoteSession.DisplayID>()
            var usedTargets = Set<AarnnRemoteSession.DisplayID>()
            for edge in snapshot.edges {
                guard let source = edge.source, let target = edge.target,
                      let from = screenByID[source], let to = screenByID[target] else { continue }
                if stage == 5 && (!usedSources.insert(source).inserted || !usedTargets.insert(target).inserted) {
                    continue
                }
                var path = Path()
                path.move(to: from)
                path.addLine(to: to)
                scene.stroke(path, with: .color(.white.opacity(0.58)), lineWidth: 1)
            }
        }

        if stage >= 7 && snapshot.volumetricClearanceVerified {
            for neurite in snapshot.paths where neurite.pointsMM.count > 1 {
                var path = Path()
                path.move(to: point(neurite.pointsMM[0]))
                for position in neurite.pointsMM.dropFirst() { path.addLine(to: point(position)) }
                let sourceID = neurite.source ?? neurite.owner
                let colour = lineColour(neurite.kind, identity: sourceID, nodeByID: nodeByID)
                let width: CGFloat = stage >= 8
                    ? max(0.5, CGFloat(2 * (neurite.radiusMM ?? 0)) * physicalScale)
                    : 1
                let isActive = (neurite.source ?? neurite.owner).map(activeNodeIDs.contains) ?? false
                scene.stroke(
                    path,
                    with: .color(isActive ? .white : colour),
                    style: StrokeStyle(lineWidth: width, lineCap: .round, lineJoin: .round)
                )
            }
        }

        if stage >= 9 && snapshot.volumetricClearanceVerified {
            for marker in snapshot.markers {
                let lower = marker.kind.lowercased()
                let colour: Color = lower.contains("bouton")
                    ? .orange
                    : lower.contains("postsynaptic") ? .cyan : .yellow
                let radius: CGFloat = lower.contains("synapse") && !lower.contains("post") ? 4.5 : 3.5
                let position = point(marker.positionMM)
                scene.fill(Path(ellipseIn: CGRect(
                    x: position.x - radius, y: position.y - radius,
                    width: radius * 2, height: radius * 2
                )), with: .color(colour))
            }
        }

        let pixelStage = stage <= 2 || (4...7).contains(stage)
        for node in snapshot.nodes {
            let position = point(node.positionMM)
            let colour = node.colourSlot.map { slotColour($0) } ?? stableColour(node.id)
            let activityColour = activeNodeIDs.contains(node.id) ? Color.white : colour.opacity(0.55)
            if pixelStage {
                scene.fill(Path(CGRect(x: position.x, y: position.y, width: 1, height: 1)), with: .color(activityColour))
            } else {
                let radius: CGFloat
                if stage == 3 {
                    radius = 1.5
                } else if stage >= 8, snapshot.volumetricClearanceVerified,
                          let somaRadius = node.somaRadiusMM, somaRadius.isFinite, somaRadius > 0 {
                    radius = max(0.5, CGFloat(somaRadius) * physicalScale)
                } else {
                    radius = 0.5
                }
                scene.fill(Path(ellipseIn: CGRect(
                    x: position.x - radius, y: position.y - radius,
                    width: radius * 2, height: radius * 2
                )), with: .color(activityColour))
            }
        }
    }

    private func lineColour(
        _ kind: String,
        identity: AarnnRemoteSession.DisplayID?,
        nodeByID: [AarnnRemoteSession.DisplayID: AarnnRemoteSession.DisplayNode]
    ) -> Color {
        let lower = kind.lowercased()
        let variant = lower.contains("axon") ? 0.055 : lower.contains("dendrite") ? -0.055 : 0
        if let slot = identity.flatMap({ nodeByID[$0]?.colourSlot }) {
            return slotColour(slot, variant: variant)
        }
        return identity.map(stableColour) ?? .white.opacity(0.6)
    }

    private func stableColour(_ identity: AarnnRemoteSession.DisplayID) -> Color {
        let text = "\(identity.value):\(identity.generation)"
        var hash: UInt64 = 14_695_981_039_346_656_037
        for byte in text.utf8 {
            hash ^= UInt64(byte)
            hash = hash &* 1_099_511_628_211
        }
        let hue = Double(hash % 3_600) / 3_600
        return Color(hue: hue, saturation: 0.72, brightness: 0.92)
    }

    private func slotColour(_ slot: UInt32, variant: Double = 0) -> Color {
        let degrees = (Double(slot) * 137.508).truncatingRemainder(dividingBy: 360)
        let hue = ((degrees / 360) + variant).truncatingRemainder(dividingBy: 1)
        return Color(hue: hue < 0 ? hue + 1 : hue, saturation: 0.648 / 0.874, brightness: 0.874)
    }
}

private extension Comparable {
    func clamped(to range: ClosedRange<Self>) -> Self {
        min(max(self, range.lowerBound), range.upperBound)
    }
}
