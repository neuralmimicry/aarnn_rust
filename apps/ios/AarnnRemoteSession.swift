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
