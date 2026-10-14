import AppKit
import ApplicationServices

@MainActor
enum WindowActivator {
    static func close(_ window: WindowInfo) {
        guard let ax = element(of: window),
              let button: AXUIElement = WindowEnumerator.attribute(ax, kAXCloseButtonAttribute)
        else { return }
        AXUIElementPerformAction(button, kAXPressAction as CFString)
    }

    private static func element(of window: WindowInfo) -> AXUIElement? {
        window.axElement ?? window.windowID.flatMap {
            WindowEnumerator.axElementByBruteForce(windowID: $0, pid: window.pid)
        }
    }

    static func activate(_ window: WindowInfo) {
        let axElement = element(of: window)

        if let ax = axElement, window.isMinimized {
            AXUIElementSetAttributeValue(ax, kAXMinimizedAttribute as CFString, kCFBooleanFalse)
        }

        let focusedBySkyLight = window.windowID.map { SkyLight.focusWindow($0, pid: window.pid) } ?? false
        if !focusedBySkyLight {
            NSRunningApplication(processIdentifier: window.pid)?.activate()
        }

        if let ax = axElement {
            AXUIElementSetAttributeValue(ax, kAXMainAttribute as CFString, kCFBooleanTrue)
            AXUIElementPerformAction(ax, kAXRaiseAction as CFString)
        }
    }
}
