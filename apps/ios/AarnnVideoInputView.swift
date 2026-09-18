import AVFoundation
import AVKit
import PhotosUI
import SwiftUI

/// The cross-product source vocabulary used by Rust UI, web, Android and CLI.
public enum AarnnVideoInputSource: String, CaseIterable, Identifiable {
    case videoFile = "video-file"
    case camera = "camera"

    public var id: String { rawValue }
}

private struct AarnnCameraChoice: Identifiable {
    let id: String
    let name: String
}

/// SwiftUI surface for an explicitly selected video input. Preview pixels are
/// presentation state; the governed media adapter remains responsible for
/// consent, clock mapping and sensory admission.
public struct AarnnVideoInputView: View {
    @State private var source: AarnnVideoInputSource = .videoFile
    @State private var selectedItem: PhotosPickerItem?
    @State private var player: AVPlayer?
    @State private var cameraPresented = false
    @State private var cameraPermission = false
    @State private var audioPermission = false
    @State private var includeAudio = false
    @State private var cameraDevices: [AarnnCameraChoice] = []
    @State private var selectedCameraID = ""
    @State private var videoHasAudio = false

    public init() {}

    public var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Picker("Video input", selection: $source) {
                ForEach(AarnnVideoInputSource.allCases) { item in
                    Text(item.rawValue).tag(item)
                }
            }
            .pickerStyle(.segmented)

            if source == .videoFile {
                PhotosPicker("Choose video", selection: $selectedItem, matching: .videos)
                    .onChange(of: selectedItem) { _, item in
                        Task { await loadVideo(item) }
                    }
            } else {
                Picker("Camera", selection: $selectedCameraID) {
                    Text("System camera").tag("")
                    ForEach(cameraDevices) { device in
                        Text(device.name).tag(device.id)
                    }
                }
                .onChange(of: selectedCameraID) { _, _ in
                    if cameraPermission { cameraPresented = false }
                }
                Button(cameraPermission ? "Pop out camera" : "Enable camera") {
                    Task { await requestCamera() }
                }
            }

            Toggle("Include microphone audio", isOn: $includeAudio)
                .onChange(of: includeAudio) { _, enabled in
                    if enabled { Task { await requestAudio() } }
                }
            Text(includeAudio || videoHasAudio
                 ? "Audio companion selected • Graphic EQ unavailable until governed spectral bands are exposed."
                 : "Camera and microphone remain separate permissions.")
                .font(.caption)
                .foregroundStyle(.secondary)

            if (source == .videoFile && player != nil) || (source == .camera && cameraPermission) {
                Button("Pop out video") { cameraPresented = true }
                    .buttonStyle(.borderedProminent)
            }
            Text("Preview pixels are display state; governed sensory admission remains separately authorised.")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .sheet(isPresented: $cameraPresented) {
            AarnnVideoPreviewSheet(
                source: source,
                player: player,
                cameraID: selectedCameraID,
                includeAudio: includeAudio,
            )
        }
        .task { refreshCameraDevices() }
    }

    private func loadVideo(_ item: PhotosPickerItem?) async {
        guard let item, let data = try? await item.loadTransferable(type: Data.self) else { return }
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("aarnn-video-input-\(UUID().uuidString).mp4")
        try? data.write(to: url, options: .atomic)
        player = AVPlayer(url: url)
        videoHasAudio = !AVAsset(url: url).tracks(withMediaType: .audio).isEmpty
    }

    private func requestCamera() async {
        cameraPermission = await AVCaptureDevice.requestAccess(for: .video)
        if cameraPermission {
            if includeAudio { await requestAudio() }
            cameraPresented = true
        }
    }

    private func requestAudio() async {
        audioPermission = await AVCaptureDevice.requestAccess(for: .audio)
    }

    private func refreshCameraDevices() {
        let discovery = AVCaptureDevice.DiscoverySession(
            deviceTypes: [.builtInWideAngleCamera, .external],
            mediaType: .video,
            position: .unspecified,
        )
        cameraDevices = discovery.devices.map { AarnnCameraChoice(id: $0.uniqueID, name: $0.localizedName) }
        if !cameraDevices.contains(where: { $0.id == selectedCameraID }) {
            selectedCameraID = cameraDevices.first?.id ?? ""
        }
    }
}

private struct AarnnVideoPreviewSheet: View {
    let source: AarnnVideoInputSource
    let player: AVPlayer?
    let cameraID: String
    let includeAudio: Bool

    var body: some View {
        NavigationStack {
            Group {
                if source == .camera {
                    AarnnCameraPreview(cameraID: cameraID, includeAudio: includeAudio)
                } else if let player {
                    VideoPlayer(player: player)
                        .onAppear { player.play() }
                } else {
                    ContentUnavailableView("No video selected", systemImage: "video")
                }
            }
            .navigationTitle("AARNN video input")
            .navigationBarTitleDisplayMode(.inline)
        }
    }
}

private struct AarnnCameraPreview: UIViewRepresentable {
    let cameraID: String
    let includeAudio: Bool

    func makeUIView(context: Context) -> PreviewView {
        let view = PreviewView()
        view.start(cameraID: cameraID, includeAudio: includeAudio)
        return view
    }

    func updateUIView(_ uiView: PreviewView, context: Context) {}

    static func dismantleUIView(_ uiView: PreviewView, coordinator: ()) {
        uiView.stop()
    }
}

private final class PreviewView: UIView {
    private let session = AVCaptureSession()

    override class var layerClass: AnyClass { AVCaptureVideoPreviewLayer.self }

    override init(frame: CGRect) {
        super.init(frame: frame)
        (layer as? AVCaptureVideoPreviewLayer)?.videoGravity = .resizeAspect
        (layer as? AVCaptureVideoPreviewLayer)?.session = session
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }

    func start(cameraID: String, includeAudio: Bool) {
        guard let device = (cameraID.isEmpty ? AVCaptureDevice.default(for: .video) : AVCaptureDevice(uniqueID: cameraID)),
              let input = try? AVCaptureDeviceInput(device: device),
              session.canAddInput(input) else { return }
        session.addInput(input)
        if includeAudio,
           let audioDevice = AVCaptureDevice.default(for: .audio),
           let audioInput = try? AVCaptureDeviceInput(device: audioDevice),
           session.canAddInput(audioInput) {
            session.addInput(audioInput)
        }
        DispatchQueue.global(qos: .userInitiated).async { self.session.startRunning() }
    }

    func stop() {
        if session.isRunning { session.stopRunning() }
    }
}
