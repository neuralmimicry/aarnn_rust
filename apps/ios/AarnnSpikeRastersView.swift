import SwiftUI

/// Read-only input and output raster over the same bounded workspace activity
/// history used by the other presentation clients.
public struct AarnnSpikeRastersView: View {
    public let activity: AarnnRemoteSession.Activity
    public let sensoryCount: Int
    public let outputCount: Int

    public init(activity: AarnnRemoteSession.Activity, sensoryCount: Int, outputCount: Int) {
        self.activity = activity
        self.sensoryCount = sensoryCount
        self.outputCount = outputCount
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Spike rasters").font(.headline)
            raster("Input", frames: activity.sensoryHistory, count: sensoryCount, colour: .mint)
            raster("Output", frames: activity.outputHistory, count: outputCount, colour: .yellow)
        }
    }

    @ViewBuilder
    private func raster(
        _ label: String,
        frames: [AarnnRemoteSession.SpikeFrame],
        count: Int,
        colour: Color
    ) -> some View {
        let ordered = Array(frames.sorted { $0.step < $1.step }.suffix(128))
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text("\(label) raster").font(.subheadline.bold())
                Spacer()
                Text("\(ordered.count) frames").font(.caption.monospacedDigit())
            }
            if ordered.isEmpty || count <= 0 {
                Text("No \(label.lowercased()) spikes yet")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            } else {
                Canvas { context, size in
                    let rows = min(64, count)
                    let columnWidth = size.width / CGFloat(ordered.count)
                    let rowHeight = size.height / CGFloat(rows)
                    for (column, frame) in ordered.enumerated() {
                        let usedRows = Set(frame.indices.filter { $0 >= 0 && $0 < count }
                            .map { $0 * rows / count })
                        for row in usedRows {
                            let rect = CGRect(
                                x: CGFloat(column) * columnWidth,
                                y: CGFloat(rows - row - 1) * rowHeight,
                                width: max(1, columnWidth),
                                height: max(1, rowHeight)
                            )
                            context.fill(Path(rect), with: .color(colour))
                        }
                    }
                }
                .frame(height: 100)
                .background(Color(red: 0.09, green: 0.09, blue: 0.09))
                .accessibilityLabel("\(label) spike raster, \(ordered.count) frames")
            }
        }
    }
}
