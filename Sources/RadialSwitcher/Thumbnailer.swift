import ScreenCaptureKit

enum Thumbnailer {
    static let pixelWidth: CGFloat = 600

    static func capture(_ windows: [WindowInfo]) async -> [CGWindowID: CGImage] {
        await capture(windows.compactMap { $0.scWindow.map(CaptureTarget.init) })
    }

    static func capture(_ targets: [CaptureTarget]) async -> [CGWindowID: CGImage] {
        await withTaskGroup(of: (CGWindowID, CGImage?).self) { group in
            for target in targets {
                group.addTask { (target.window.windowID, await captureImage(of: target.window)) }
            }
            var images: [CGWindowID: CGImage] = [:]
            for await (id, image) in group {
                images[id] = image
            }
            return images
        }
    }

    private static func captureImage(of scWindow: SCWindow) async -> CGImage? {
        guard scWindow.frame.width > 0, scWindow.frame.height > 0 else { return nil }

        let config = SCStreamConfiguration()
        let width = min(pixelWidth, scWindow.frame.width * 2)
        config.width = Int(width)
        config.height = Int(width * scWindow.frame.height / scWindow.frame.width)
        config.showsCursor = false
        config.ignoreShadowsSingleWindow = true

        let filter = SCContentFilter(desktopIndependentWindow: scWindow)
        return try? await SCScreenshotManager.captureImage(contentFilter: filter, configuration: config)
    }
}

struct CaptureTarget: @unchecked Sendable {
    let window: SCWindow
}
