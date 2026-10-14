import ScreenCaptureKit
import VideoToolbox

@MainActor
final class LivePreview {
    var onFrame: ((CGWindowID, CGImage) -> Void)?

    private(set) var windowID: CGWindowID?
    private var stream: SCStream?
    private var output: FrameOutput?
    private let queue = DispatchQueue(label: "live-preview", qos: .userInteractive)

    static let framesPerSecond: Int32 = 30

    func start(_ window: WindowInfo) {
        guard let id = window.windowID, let scWindow = window.scWindow, id != windowID else { return }
        stop()

        let config = SCStreamConfiguration()
        let width = min(Thumbnailer.pixelWidth, scWindow.frame.width * 2)
        config.width = Int(width)
        config.height = Int(width * scWindow.frame.height / max(scWindow.frame.width, 1))
        config.minimumFrameInterval = CMTime(value: 1, timescale: Self.framesPerSecond)
        config.pixelFormat = kCVPixelFormatType_32BGRA
        config.queueDepth = 3
        config.showsCursor = false
        config.ignoreShadowsSingleWindow = true

        let stream = SCStream(filter: SCContentFilter(desktopIndependentWindow: scWindow), configuration: config, delegate: nil)
        let output = FrameOutput(windowID: id) { [weak self] id, image in
            Task { @MainActor in
                guard let self, self.windowID == id else { return }
                self.onFrame?(id, image)
            }
        }
        do {
            try stream.addStreamOutput(output, type: .screen, sampleHandlerQueue: queue)
        } catch {
            print("Live preview: \(error.localizedDescription)")
            return
        }

        self.stream = stream
        self.output = output
        windowID = id
        Task {
            do { try await stream.startCapture() } catch { print("Live preview: \(error.localizedDescription)") }
        }
    }

    func stop() {
        guard let stream else { return }
        self.stream = nil
        output = nil
        windowID = nil
        Task { try? await stream.stopCapture() }
    }
}

private final class FrameOutput: NSObject, SCStreamOutput, @unchecked Sendable {
    private let windowID: CGWindowID
    private let onFrame: @Sendable (CGWindowID, CGImage) -> Void

    init(windowID: CGWindowID, onFrame: @escaping @Sendable (CGWindowID, CGImage) -> Void) {
        self.windowID = windowID
        self.onFrame = onFrame
    }

    func stream(_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .screen,
              let attachments = CMSampleBufferGetSampleAttachmentsArray(sampleBuffer, createIfNecessary: false) as? [[SCStreamFrameInfo: Any]],
              let rawStatus = attachments.first?[.status] as? Int,
              SCFrameStatus(rawValue: rawStatus) == .complete,
              let pixelBuffer = CMSampleBufferGetImageBuffer(sampleBuffer)
        else { return }

        var image: CGImage?
        VTCreateCGImageFromCVPixelBuffer(pixelBuffer, options: nil, imageOut: &image)
        if let image { onFrame(windowID, image) }
    }
}
