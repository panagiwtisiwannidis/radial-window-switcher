import AppKit
import ApplicationServices
import ScreenCaptureKit

@_silgen_name("_AXUIElementGetWindow")
private func _AXUIElementGetWindow(_ element: AXUIElement, _ windowID: UnsafeMutablePointer<CGWindowID>) -> AXError

struct WindowInfo {
    let appName: String
    let pid: pid_t
    let bundleID: String?
    let windowID: CGWindowID?
    let title: String
    let isMinimized: Bool
    let frame: CGRect?
    var scWindow: SCWindow?
    var isCapturable: Bool { scWindow != nil }
    let axElement: AXUIElement?
    let isOnOtherSpace: Bool
}

extension WindowInfo: @unchecked Sendable {}

@MainActor
enum WindowEnumerator {
    static func listWindows(useScreenCaptureKit: Bool) async throws -> [WindowInfo] {
        let apps = NSWorkspace.shared.runningApplications.filter { $0.activationPolicy == .regular }
        let appInfos = apps.map { AppRef(pid: $0.processIdentifier, name: $0.localizedName ?? "?", bundleID: $0.bundleIdentifier) }

        var windows = await Task.detached { axWindowsOfAll(appInfos) }.value

        guard useScreenCaptureKit else { return windows }

        let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: false)
        let scWindowsByID = Dictionary(content.windows.map { ($0.windowID, $0) }, uniquingKeysWith: { a, _ in a })

        for i in windows.indices {
            if let id = windows[i].windowID {
                windows[i].scWindow = scWindowsByID[id]
            }
        }

        let axIDs = Set(windows.compactMap(\.windowID))
        let regularPIDs = Set(apps.map(\.processIdentifier))
        let activeSpace = SkyLight.activeSpace()
        for scWindow in content.windows
        where scWindow.windowLayer == 0
            && !axIDs.contains(scWindow.windowID)
            && regularPIDs.contains(scWindow.owningApplication?.processID ?? -1)
            && !(scWindow.title ?? "").isEmpty
        {
            let spaces = SkyLight.spaces(of: scWindow.windowID)
            guard let activeSpace, !spaces.isEmpty, !spaces.contains(activeSpace) else { continue }

            windows.append(WindowInfo(
                appName: scWindow.owningApplication?.applicationName ?? "?",
                pid: scWindow.owningApplication?.processID ?? -1,
                bundleID: scWindow.owningApplication?.bundleIdentifier,
                windowID: scWindow.windowID,
                title: scWindow.title ?? "",
                isMinimized: false,
                frame: scWindow.frame,
                scWindow: scWindow,
                axElement: nil,
                isOnOtherSpace: true
            ))
        }

        return windows
    }

    private struct AppRef: Sendable {
        let pid: pid_t
        let name: String
        let bundleID: String?
    }

    nonisolated private static func axWindowsOfAll(_ apps: [AppRef]) -> [WindowInfo] {
        final class Results: @unchecked Sendable {
            let lock = NSLock()
            var perApp: [[WindowInfo]]
            init(count: Int) { perApp = Array(repeating: [], count: count) }
        }
        let results = Results(count: apps.count)
        DispatchQueue.concurrentPerform(iterations: apps.count) { i in
            let found = axWindows(of: apps[i])
            results.lock.withLock { results.perApp[i] = found }
        }
        return results.perApp.flatMap { $0 }
    }

    nonisolated private static func axWindows(of app: AppRef) -> [WindowInfo] {
        let appElement = AXUIElementCreateApplication(app.pid)
        AXUIElementSetMessagingTimeout(appElement, 0.5)

        guard let axWindows: [AXUIElement] = attribute(appElement, kAXWindowsAttribute) else { return [] }

        return axWindows.compactMap { window in
            let subrole: String? = attribute(window, kAXSubroleAttribute)
            let isMinimized: Bool = attribute(window, kAXMinimizedAttribute) ?? false
            guard subrole == kAXStandardWindowSubrole as String || isMinimized else { return nil }

            var windowID: CGWindowID = 0
            let hasID = _AXUIElementGetWindow(window, &windowID) == .success

            return WindowInfo(
                appName: app.name,
                pid: app.pid,
                bundleID: app.bundleID,
                windowID: hasID ? windowID : nil,
                title: attribute(window, kAXTitleAttribute) ?? "",
                isMinimized: isMinimized,
                frame: frame(of: window),
                scWindow: nil,
                axElement: window,
                isOnOtherSpace: false
            )
        }
    }

    static func axElementByBruteForce(windowID: CGWindowID, pid: pid_t) -> AXUIElement? {
        guard let createFromToken = remoteTokenFactory else { return nil }
        var token = Data(count: 20)
        token.replaceSubrange(0..<4, with: withUnsafeBytes(of: pid) { Data($0) })
        token.replaceSubrange(4..<8, with: withUnsafeBytes(of: Int32(0)) { Data($0) })
        token.replaceSubrange(8..<12, with: withUnsafeBytes(of: Int32(0x636f_636f)) { Data($0) })

        let deadline = Date().addingTimeInterval(0.1)
        for elementID in UInt64(0)..<1000 {
            if Date() > deadline { break }
            token.replaceSubrange(12..<20, with: withUnsafeBytes(of: elementID) { Data($0) })
            guard let element = createFromToken(token as CFData)?.takeRetainedValue() else { continue }
            var id: CGWindowID = 0
            if _AXUIElementGetWindow(element, &id) == .success, id == windowID {
                return element
            }
        }
        return nil
    }

    private typealias CreateWithRemoteTokenFn = @convention(c) (CFData) -> Unmanaged<AXUIElement>?
    private static let remoteTokenFactory: CreateWithRemoteTokenFn? = {
        guard let ptr = dlsym(UnsafeMutableRawPointer(bitPattern: -2), "_AXUIElementCreateWithRemoteToken") else { return nil }
        return unsafeBitCast(ptr, to: CreateWithRemoteTokenFn.self)
    }()

    nonisolated static func attribute<T>(_ element: AXUIElement, _ name: String) -> T? {
        var value: CFTypeRef?
        guard AXUIElementCopyAttributeValue(element, name as CFString, &value) == .success else { return nil }
        return value as? T
    }

    nonisolated private static func frame(of window: AXUIElement) -> CGRect? {
        guard let posValue: AXValue = attribute(window, kAXPositionAttribute),
              let sizeValue: AXValue = attribute(window, kAXSizeAttribute) else { return nil }
        var origin = CGPoint.zero
        var size = CGSize.zero
        AXValueGetValue(posValue, .cgPoint, &origin)
        AXValueGetValue(sizeValue, .cgSize, &size)
        return CGRect(origin: origin, size: size)
    }
}
