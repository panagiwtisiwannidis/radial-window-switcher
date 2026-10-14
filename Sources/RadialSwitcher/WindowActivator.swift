import AppKit
import ApplicationServices

@MainActor
enum WindowActivator {
    static func activate(_ window: WindowInfo) {
        let axElement = window.axElement ?? window.windowID.flatMap {
            WindowEnumerator.axElementByBruteForce(windowID: $0, pid: window.pid)
        }

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
