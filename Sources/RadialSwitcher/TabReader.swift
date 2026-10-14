import ApplicationServices
import Foundation

struct TabInfo: @unchecked Sendable {
    let title: String
    let isActive: Bool
    let element: AXUIElement
}

enum TabReader {
    private static let maxDepth = 5
    private static let skippedRoles: Set<String> = [
        "AXScrollArea", "AXWebArea", "AXTextArea", "AXTable", "AXOutline", "AXBrowser", "AXList", "AXMenuBar",
    ]

    static func tabs(of windows: [WindowInfo]) -> [CGWindowID: [TabInfo]] {
        let targets = windows.filter { $0.windowID != nil && $0.axElement != nil }
        final class Results: @unchecked Sendable {
            let lock = NSLock()
            var byWindow: [CGWindowID: [TabInfo]] = [:]
        }
        let results = Results()
        DispatchQueue.concurrentPerform(iterations: targets.count) { i in
            guard let id = targets[i].windowID, let element = targets[i].axElement else { return }
            let found = tabs(in: element)
            guard found.count >= 2 else { return }
            results.lock.withLock { results.byWindow[id] = found }
        }
        return results.byWindow
    }

    static func select(_ tab: TabInfo) {
        AXUIElementPerformAction(tab.element, kAXPressAction as CFString)
    }

    private static func tabs(in window: AXUIElement) -> [TabInfo] {
        var found: [TabInfo] = []
        var queue: [(element: AXUIElement, depth: Int)] = [(window, 0)]
        while !queue.isEmpty {
            let (element, depth) = queue.removeFirst()
            let role: String = WindowEnumerator.attribute(element, kAXRoleAttribute) ?? ""
            let subrole: String? = WindowEnumerator.attribute(element, kAXSubroleAttribute)

            if role == kAXRadioButtonRole as String, subrole == "AXTabButton" {
                let title: String = WindowEnumerator.attribute(element, kAXTitleAttribute) ?? ""
                let value: Int = WindowEnumerator.attribute(element, kAXValueAttribute) ?? 0
                found.append(TabInfo(title: title, isActive: value == 1, element: element))
                continue
            }
            guard depth < maxDepth, !skippedRoles.contains(role),
                  let children: [AXUIElement] = WindowEnumerator.attribute(element, kAXChildrenAttribute)
            else { continue }
            queue += children.map { ($0, depth + 1) }
        }
        return found
    }
}
