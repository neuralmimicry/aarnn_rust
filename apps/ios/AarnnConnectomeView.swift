import SwiftUI

/// Shared-contract connectome view for the future iOS shell. Both presets use
/// the same stable node identities; anatomical mode draws stored centre-lines
/// and synthetic mode draws the contract's column positions and graph edges.
public struct AarnnConnectomeView: View {
    public let views: AarnnRemoteSession.DisplayViews
    public let activeNodeIDs: Set<AarnnRemoteSession.DisplayID>
    @State private var mode: AarnnRemoteSession.DisplayMode = .anatomical

    public init(
        views: AarnnRemoteSession.DisplayViews,
        activeNodeIDs: Set<AarnnRemoteSession.DisplayID> = [],
    ) {
        self.views = views
        self.activeNodeIDs = activeNodeIDs
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Picker("Display", selection: $mode) {
                Text("Synthetic columns").tag(AarnnRemoteSession.DisplayMode.syntheticColumns)
                Text("Anatomical").tag(AarnnRemoteSession.DisplayMode.anatomical)
            }
            .pickerStyle(.segmented)

            if let snapshot = views.snapshot(for: mode) {
                Canvas { context, size in
                    draw(snapshot: snapshot, in: &context, size: size)
                }
                .background(Color.black.opacity(0.9))
                .clipShape(RoundedRectangle(cornerRadius: 12))
                Text("\(snapshot.provenance) • sequence \(snapshot.sequence)\(snapshot.truncated ? " • bounded" : "")")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            } else {
                ContentUnavailableView("Display unavailable", systemImage: "point.3.connected.trianglepath.dotted")
            }
        }
        .padding()
    }

    private func draw(
        snapshot: AarnnRemoteSession.DisplaySnapshot,
        in context: inout GraphicsContext,
        size: CGSize,
    ) {
        let allPoints = snapshot.nodes.map(\.positionMM)
            + snapshot.edges.flatMap { $0.pointsMM }
            + snapshot.paths.flatMap { $0.pointsMM }
            + snapshot.markers.map(\.positionMM)
        let minX = snapshot.region?.min.x ?? allPoints.map(\.x).min() ?? -1
        let maxX = snapshot.region?.max.x ?? allPoints.map(\.x).max() ?? 1
        let minY = snapshot.region?.min.y ?? allPoints.map(\.y).min() ?? -1
        let maxY = snapshot.region?.max.y ?? allPoints.map(\.y).max() ?? 1
        let spanX = max(maxX - minX, 0.000001)
        let spanY = max(maxY - minY, 0.000001)

        func point(_ value: AarnnRemoteSession.DisplayPoint) -> CGPoint {
            CGPoint(
                x: 14 + (value.x - minX) / spanX * max(1, size.width - 28),
                y: size.height - 14 - (value.y - minY) / spanY * max(1, size.height - 28),
            )
        }

        if snapshot.mode == .anatomical, let region = snapshot.region {
            let minimum: CGPoint
            let maximum: CGPoint
            if let membrane = snapshot.membrane {
                minimum = point(.init(x: membrane.centreMM.x - membrane.radiiMM.x, y: membrane.centreMM.y - membrane.radiiMM.y, z: membrane.centreMM.z - membrane.radiiMM.z))
                maximum = point(.init(x: membrane.centreMM.x + membrane.radiiMM.x, y: membrane.centreMM.y + membrane.radiiMM.y, z: membrane.centreMM.z + membrane.radiiMM.z))
            } else {
                minimum = point(region.min)
                maximum = point(region.max)
            }
            let membraneRect = CGRect(
                x: min(minimum.x, maximum.x), y: min(minimum.y, maximum.y),
                width: abs(maximum.x - minimum.x), height: abs(maximum.y - minimum.y)
            )
            // The region is an ellipsoid bounding box only when the server
            // supplies an actual membrane. Imported box environments stay boxes.
            let membranePath = snapshot.membrane == nil ? Path(membraneRect) : Path(ellipseIn: membraneRect)
            context.fill(membranePath, with: .color(Color(red: 0.47, green: 0.57, blue: 0.75).opacity(0.10)))
            context.clip(to: membranePath)
        }

        func stableColour(_ identity: AarnnRemoteSession.DisplayID?, variant: Double = 0) -> Color {
            guard let identity else { return .white.opacity(0.6) }
            let text = "\(identity.value):\(identity.generation)"
            var hash: UInt64 = 14_695_981_039_346_656_037
            for byte in text.utf8 {
                hash ^= UInt64(byte)
                hash = hash &* 1_099_511_628_211
            }
            let hue = ((Double(hash % 3_600) / 10.0) + variant * 360.0).truncatingRemainder(dividingBy: 360.0) / 360.0
            return Color(hue: hue < 0 ? hue + 1 : hue, saturation: 0.72, brightness: 0.92)
        }

        func slotColour(_ slot: UInt32?, variant: Double = 0) -> Color {
            guard let slot else { return .white.opacity(0.6) }
            let degrees = (Double(slot) * 137.508).truncatingRemainder(dividingBy: 360.0)
            let hue = ((degrees / 360.0) + variant).truncatingRemainder(dividingBy: 1.0)
            // HSL(72%, 55%) expressed as HSV, matching CSS and Compose.
            return Color(hue: hue < 0 ? hue + 1 : hue, saturation: 0.648 / 0.874, brightness: 0.874)
        }

        let slotByID = Dictionary(uniqueKeysWithValues: snapshot.nodes.map { node in
            ("\(node.id.value):\(node.id.generation)", node.colourSlot)
        })

        func colour(_ kind: String, identity: AarnnRemoteSession.DisplayID?) -> Color {
            let lower = kind.lowercased()
            let key = identity.map { "\($0.value):\($0.generation)" }
            let slot = key.flatMap { slotByID[$0] } ?? nil
            if lower.contains("axon") { return slotColour(slot, variant: 0.055) }
            if lower.contains("dendrite") { return slotColour(slot, variant: -0.055) }
            if lower.contains("route") { return slotColour(slot) }
            return .blue.opacity(0.6)
        }

        for line in snapshot.paths + snapshot.edges {
            guard line.pointsMM.count > 1 else { continue }
            let kind = line.kind.lowercased()
            let isNeurite = kind.contains("axon") || kind.contains("dendrite")
            if snapshot.mode == .anatomical && !isNeurite { continue }
            let projected = line.pointsMM.map(point)
            if snapshot.mode == .anatomical, isNeurite, let radiusMM = line.radiusMM, radiusMM > 0 {
                let halfWidth = max(1.8, min(9.0, radiusMM / max(spanX, spanY) * min(size.width, size.height) * 2.0))
                var polygon = Path()
                for (a, b) in zip(projected, projected.dropFirst()) {
                    let dx = b.x - a.x, dy = b.y - a.y
                    let length = (dx * dx + dy * dy).squareRoot()
                    guard length > 0.0001 else { continue }
                    let nx = -dy / length * halfWidth, ny = dx / length * halfWidth
                    polygon.move(to: CGPoint(x: a.x + nx, y: a.y + ny))
                    polygon.addLine(to: CGPoint(x: a.x - nx, y: a.y - ny))
                    polygon.addLine(to: CGPoint(x: b.x - nx, y: b.y - ny))
                    polygon.addLine(to: CGPoint(x: b.x + nx, y: b.y + ny))
                    polygon.closeSubpath()
                }
                context.fill(polygon, with: .color(colour(line.kind, identity: line.source ?? line.owner).opacity(0.94)))
                if let owner = line.source ?? line.owner, activeNodeIDs.contains(owner) {
                    context.fill(polygon, with: .color(.white.opacity(0.38)))
                }
                continue
            }
            var path = Path()
            path.move(to: projected[0])
            for item in projected.dropFirst() { path.addLine(to: item) }
            let width: CGFloat = kind.contains("axon") || kind.contains("dendrite")
                ? 2.0
                : kind.contains("route") ? 1.5 : 1.2
            context.stroke(path, with: .color(colour(line.kind, identity: line.source ?? line.owner).opacity(kind.contains("route") ? 0.58 : 0.95)), lineWidth: width)
        }

        for marker in snapshot.markers {
            let position = point(marker.positionMM)
            let lower = marker.kind.lowercased()
            let colour: Color = lower.contains("bouton")
                ? .orange
                : lower.contains("postsynaptic") ? .cyan : .yellow
            let radius: CGFloat = lower.contains("synapse") && !lower.contains("post") ? 4.5 : 3.5
            context.fill(
                Path(ellipseIn: CGRect(
                    x: position.x - radius,
                    y: position.y - radius,
                    width: radius * 2,
                    height: radius * 2,
                )),
                with: .color(colour),
            )
        }

        for node in snapshot.nodes {
            let position = point(node.positionMM)
            let colour = node.colourSlot.map { slotColour($0) } ?? stableColour(node.id)
            let somaRadius: CGFloat = snapshot.mode == .anatomical ? 6 : 3
            context.fill(Path(ellipseIn: CGRect(x: position.x - somaRadius, y: position.y - somaRadius, width: somaRadius * 2, height: somaRadius * 2)), with: .color(activeNodeIDs.contains(node.id) ? .white : colour))
        }
    }
}
