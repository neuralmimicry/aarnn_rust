import Foundation

/// Authenticated remote workspace client shared by the iOS shell and its
/// previews. The web gateway owns the session cookie and orchestrator
/// authorisation; this client never dials a worker directly.
public actor AarnnRemoteSession {
    public struct Workspace: Decodable, Sendable {
        public let ownerID: String
        public let workspaceID: String
        public let networkID: String
        public let name: String
        public let running: Bool
        public let totalNeurons: Int
        public let distributedNodeCount: Int

        enum CodingKeys: String, CodingKey {
            case ownerID = "owner_id"
            case workspaceID = "workspace_id"
            case networkID = "network_id"
            case name, running
            case totalNeurons = "total_neurons"
            case distributedNodeCount = "distributed_node_count"
        }
    }

    public struct DisplayViews: Decodable, Sendable {
        public let syntheticColumns: DisplaySnapshot?
        public let anatomical: DisplaySnapshot?

        public init(syntheticColumns: DisplaySnapshot? = nil, anatomical: DisplaySnapshot? = nil) {
            self.syntheticColumns = syntheticColumns
            self.anatomical = anatomical
        }

        public func snapshot(for mode: DisplayMode) -> DisplaySnapshot? {
            mode == .anatomical ? anatomical : syntheticColumns
        }

        enum CodingKeys: String, CodingKey {
            case syntheticColumns = "synthetic_columns"
            case anatomical
        }
    }

    public enum DisplayMode: String, CaseIterable, Hashable, Sendable {
        case anatomical = "Anatomical"
        case syntheticColumns = "SyntheticColumns"
    }

    public struct DisplayPoint: Decodable, Sendable {
        public let x: Double
        public let y: Double
        public let z: Double
    }

    public struct DisplayRegion: Decodable, Sendable {
        public let min: DisplayPoint
        public let max: DisplayPoint
    }

    public struct DisplayID: Decodable, Sendable, Hashable {
        public let value: UInt64
        public let generation: UInt32

        enum CodingKeys: String, CodingKey { case value, generation }

        public init(from decoder: Decoder) throws {
            let values = try decoder.container(keyedBy: CodingKeys.self)
            if let decimal = try? values.decode(String.self, forKey: .value) {
                guard let decoded = UInt64(decimal) else {
                    throw DecodingError.dataCorruptedError(forKey: .value, in: values, debugDescription: "Invalid anatomical identity")
                }
                value = decoded
            } else {
                value = try values.decode(UInt64.self, forKey: .value)
            }
            generation = try values.decode(UInt32.self, forKey: .generation)
        }
    }

    public struct DisplayNode: Decodable, Sendable {
        public let id: DisplayID
        public let role: String
        public let layer: Int?
        public let positionMM: DisplayPoint
        public let colourSlot: UInt32?

        enum CodingKeys: String, CodingKey {
            case id, role, layer
            case positionMM = "position_mm"
            case colourSlot = "colour_slot"
        }
    }

    public struct DisplayLine: Decodable, Sendable {
        public let source: DisplayID?
        public let target: DisplayID?
        public let owner: DisplayID?
        public let kind: String
        public let pointsMM: [DisplayPoint]
        public let radiusMM: Double?

        enum CodingKeys: String, CodingKey {
            case source, target, owner, kind
            case pointsMM = "points_mm"
            case radiusMM = "radius_mm"
        }
    }

    public struct DisplayMarker: Decodable, Sendable {
        public let id: DisplayID
        public let owner: DisplayID
        public let kind: String
        public let positionMM: DisplayPoint
        public let synapseID: DisplayID?

        enum CodingKeys: String, CodingKey {
            case id, owner, kind
            case positionMM = "position_mm"
            case synapseID = "synapse_id"
        }
    }

    public struct DisplayMembrane: Decodable, Sendable {
        public let centreMM: DisplayPoint
        public let radiiMM: DisplayPoint
        enum CodingKeys: String, CodingKey {
            case centreMM = "centre_mm"
            case radiiMM = "radii_mm"
        }
    }

    public struct DisplaySnapshot: Decodable, Sendable {
        public let schemaVersion: UInt16
        public let morphologyRevision: UInt64
        public let topologyEpoch: UInt64
        public let routeEpoch: UInt64
        public let sequence: UInt64
        public let mode: DisplayMode
        public let provenance: String
        public let complete: Bool
        public let truncated: Bool
        public let unavailableReason: String?
        public let region: DisplayRegion?
        public let membrane: DisplayMembrane?
        public let nodes: [DisplayNode]
        public let edges: [DisplayLine]
        public let paths: [DisplayLine]
        public let markers: [DisplayMarker]

        enum CodingKeys: String, CodingKey {
            case schemaVersion = "schema_version"
            case morphologyRevision = "morphology_revision"
            case topologyEpoch = "topology_epoch"
            case routeEpoch = "route_epoch"
            case sequence, mode, provenance, nodes, edges, paths, markers, coverage
        }

        private struct Coverage: Decodable {
            let complete: Bool
            let truncated: Bool
            let unavailableReason: String?
            let region: DisplayRegion?
            let membrane: DisplayMembrane?

            enum CodingKeys: String, CodingKey {
                case complete, truncated
                case unavailableReason = "unavailable_reason"
                case region, membrane
            }
        }

        public init(from decoder: Decoder) throws {
            let values = try decoder.container(keyedBy: CodingKeys.self)
            schemaVersion = try values.decode(UInt16.self, forKey: .schemaVersion)
            morphologyRevision = try values.decode(UInt64.self, forKey: .morphologyRevision)
            topologyEpoch = try values.decode(UInt64.self, forKey: .topologyEpoch)
            routeEpoch = try values.decode(UInt64.self, forKey: .routeEpoch)
            sequence = try values.decode(UInt64.self, forKey: .sequence)
            mode = try values.decode(DisplayMode.self, forKey: .mode)
            provenance = try values.decode(String.self, forKey: .provenance)
            nodes = try values.decode([DisplayNode].self, forKey: .nodes)
            edges = try values.decodeIfPresent([DisplayLine].self, forKey: .edges) ?? []
            paths = try values.decodeIfPresent([DisplayLine].self, forKey: .paths) ?? []
            markers = try values.decodeIfPresent([DisplayMarker].self, forKey: .markers) ?? []
            let coverage = try values.decodeIfPresent(Coverage.self, forKey: .coverage)
            complete = coverage?.complete ?? false
            truncated = coverage?.truncated ?? false
            unavailableReason = coverage?.unavailableReason
            region = coverage?.region
            membrane = coverage?.membrane
        }
    }

    private struct WorkspaceSnapshotResponse: Decodable {
        let displaySnapshots: DisplayViews

        enum CodingKeys: String, CodingKey {
            case displaySnapshots = "display_snapshots"
        }

        init(from decoder: Decoder) throws {
            let values = try decoder.container(keyedBy: CodingKeys.self)
            displaySnapshots = try values.decodeIfPresent(DisplayViews.self, forKey: .displaySnapshots) ?? DisplayViews()
        }
    }

    public enum SessionError: Error, LocalizedError {
        case invalidEndpoint
        case loginRejected
        case unexpectedResponse

        public var errorDescription: String? {
            switch self {
            case .invalidEndpoint: return "The remote AARNN endpoint is invalid."
            case .loginRejected: return "The remote AARNN login was rejected."
            case .unexpectedResponse: return "The remote AARNN gateway returned an unexpected response."
            }
        }
    }

    private struct LoginRequest: Encodable {
        let username: String
        let password: String
    }

    private struct LoginResponse: Decodable {
        let authenticated: Bool
    }

    private let endpoint: URL
    private let session: URLSession

    public init(endpoint: String, configuration: URLSessionConfiguration = .ephemeral) throws {
        guard let url = URL(string: endpoint.trimmingCharacters(in: .whitespacesAndNewlines).trimmingCharacters(in: CharacterSet(charactersIn: "/"))),
              let scheme = url.scheme?.lowercased(), scheme == "https" || scheme == "http" else {
            throw SessionError.invalidEndpoint
        }
        self.endpoint = url
        self.session = URLSession(configuration: configuration)
    }

    public func login(username: String, password: String) async throws {
        var request = try request(path: "/api/login", method: "POST")
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONEncoder().encode(LoginRequest(username: username, password: password))
        let (data, response) = try await session.data(for: request)
        guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
            throw SessionError.loginRejected
        }
        guard try JSONDecoder().decode(LoginResponse.self, from: data).authenticated else {
            throw SessionError.loginRejected
        }
    }

    public func listWorkspaces() async throws -> [Workspace] {
        let (data, response) = try await session.data(for: request(path: "/api/runtime/workspaces"))
        try validate(response)
        return try JSONDecoder().decode([Workspace].self, from: data)
    }

    public func sharedSystemWorkspace() async throws -> Workspace {
        guard let workspace = try await listWorkspaces().first(where: {
            $0.networkID == "neuralmimicry-shared-snn" && $0.ownerID.lowercased() == "system"
        }) else {
            throw SessionError.unexpectedResponse
        }
        return workspace
    }

    public func workspaceTopology(workspaceID: String, ownerID: String) async throws -> Data {
        var components = URLComponents(url: endpoint.appendingPathComponent("api/runtime/workspaces/\(workspaceID)/topology"), resolvingAgainstBaseURL: false)
        components?.queryItems = [
            URLQueryItem(name: "owner", value: ownerID),
            URLQueryItem(name: "max_nodes", value: "512"),
            URLQueryItem(name: "max_edges", value: "4096"),
        ]
        guard let url = components?.url else { throw SessionError.invalidEndpoint }
        let (data, response) = try await session.data(for: URLRequest(url: url))
        try validate(response)
        return data
    }

    /// Fetches the same bounded, versioned presentation contract consumed by
    /// the native Rust and web clients. It is derived display state and never
    /// becomes authoritative neural or morphological state.
    public func workspaceDisplayViews(workspaceID: String, ownerID: String) async throws -> DisplayViews {
        var components = URLComponents(url: endpoint.appendingPathComponent("api/runtime/workspaces/\(workspaceID)/snapshot"), resolvingAgainstBaseURL: false)
        components?.queryItems = [URLQueryItem(name: "owner", value: ownerID)]
        guard let url = components?.url else { throw SessionError.invalidEndpoint }
        let (data, response) = try await session.data(for: URLRequest(url: url))
        try validate(response)
        return try JSONDecoder().decode(WorkspaceSnapshotResponse.self, from: data).displaySnapshots
    }

    private func request(path: String, method: String = "GET") throws -> URLRequest {
        guard let url = URL(string: path, relativeTo: endpoint)?.absoluteURL else {
            throw SessionError.invalidEndpoint
        }
        var request = URLRequest(url: url)
        request.httpMethod = method
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        return request
    }

    private func validate(_ response: URLResponse) throws {
        guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
            throw SessionError.unexpectedResponse
        }
    }
}
