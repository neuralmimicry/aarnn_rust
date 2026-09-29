import Foundation

/// Presentation-only detail levels shared with the other AARNN clients.
/// Activity and render timing never feed back into neural execution.
enum AarnnVisualizationPolicy {
    static let version = 1
    static let upshiftDwellMilliseconds: UInt64 = 1_500
    static let stages = [
        "Synthetic pixels", "Synthetic connections", "Synthetic neurons",
        "Anatomical pixels", "Straight connections", "Branching connections",
        "Anatomical centre-lines", "Anatomical volumes", "Synapses and boutons",
    ]

    private static let zoomUpperBounds = [0.45, 0.65, 0.90, 1.20, 1.55, 1.95, 2.50, 3.20]
    private static let latencyUpperBoundsMilliseconds = [5.0, 8.0, 12.0, 16.0, 24.0, 33.0, 50.0, 80.0]

    static func zoomStage(_ zoom: Double) -> Int {
        guard zoom.isFinite else { return 1 }
        return (zoomUpperBounds.firstIndex(where: { zoom < $0 }) ?? 8) + 1
    }

    static func latencyStage(_ latencyMilliseconds: Double) -> Int {
        guard latencyMilliseconds.isFinite else { return 1 }
        for (index, boundary) in latencyUpperBoundsMilliseconds.enumerated() where latencyMilliseconds <= boundary {
            return 9 - index
        }
        return 1
    }

    /// Drop detail promptly and raise it one stage only after a sustained dwell.
    static func update(
        current: Int,
        zoom: Double,
        latencyMilliseconds: Double,
        highestAvailable: Int,
        dwellMilliseconds: UInt64
    ) -> Int {
        let stage = min(9, max(1, current))
        let maximum = min(9, max(1, highestAvailable))
        let target = min(zoomStage(zoom), latencyStage(latencyMilliseconds), maximum)
        if target < stage { return target }
        if target > stage && dwellMilliseconds >= upshiftDwellMilliseconds { return stage + 1 }
        return min(stage, maximum)
    }

    /// Resolve only geometry justified by both physical radii and a clearance witness.
    static func highestAvailable(
        synthetic: AarnnRemoteSession.DisplaySnapshot?,
        anatomical: AarnnRemoteSession.DisplaySnapshot?
    ) -> Int {
        guard let anatomical, !anatomical.nodes.isEmpty else {
            return synthetic?.nodes.isEmpty == false ? 3 : 1
        }
        var highest = anatomical.edges.isEmpty ? 4 : 6
        guard anatomical.volumetricClearanceVerified, !anatomical.paths.isEmpty else { return highest }
        let physicalSomas = anatomical.nodes.allSatisfy {
            guard let radius = $0.somaRadiusMM else { return false }
            return radius.isFinite && radius > 0
        }
        let physicalNeurites = anatomical.paths.allSatisfy { path in
            guard let radius = path.radiusMM else { return false }
            return radius.isFinite && radius > 0
        }
        if physicalSomas && physicalNeurites {
            highest = anatomical.markers.isEmpty ? 8 : 9
        }
        return highest
    }
}

/// A bounded, thread-safe p95 sample of renderer work, isolated from the brain runner.
final class VisualizationLatencyTracker: @unchecked Sendable {
    private let lock = NSLock()
    private let capacity: Int
    private var samples: [Double] = []

    init(capacity: Int = 32) {
        self.capacity = max(1, capacity)
    }

    func record(milliseconds: Double) {
        guard milliseconds.isFinite, milliseconds >= 0 else { return }
        lock.lock()
        defer { lock.unlock() }
        samples.append(milliseconds)
        if samples.count > capacity {
            samples.removeFirst(samples.count - capacity)
        }
    }

    func p95Milliseconds() -> Double {
        lock.lock()
        let captured = samples
        lock.unlock()
        guard !captured.isEmpty else { return 16 }
        let ordered = captured.sorted()
        let index = min(ordered.count - 1, ((ordered.count - 1) * 95) / 100)
        return ordered[index]
    }
}
