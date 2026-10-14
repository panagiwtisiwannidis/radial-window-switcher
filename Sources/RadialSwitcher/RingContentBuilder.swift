import AppKit

@MainActor
enum RingContentBuilder {
    static func build(windows: [WindowInfo], frontmost: NSRunningApplication?) -> (inner: [RingItem], outer: [RingItem]) {
        let windowsByPID = Dictionary(grouping: windows, by: \.pid)
        let frontPID = frontmost?.processIdentifier

        let inner = (frontPID.flatMap { windowsByPID[$0] } ?? [])
            .prefix(RingGeometry.maxInnerItems)
            .map { windowItem($0, app: frontmost) }

        let otherApps = NSWorkspace.shared.runningApplications.filter {
            $0.activationPolicy == .regular
                && $0.processIdentifier != frontPID
                && $0.processIdentifier != getpid()
        }
        let outer = sortedByRecency(otherApps)
            .map { appItem($0, windows: windowsByPID[$0.processIdentifier] ?? []) }

        return (Array(inner), outer)
    }

    private static func windowItem(_ window: WindowInfo, app: NSRunningApplication?) -> RingItem {
        let title = window.title.isEmpty ? window.appName : window.title
        return RingItem(
            id: "w\(window.windowID ?? 0)-\(window.pid)-\(title)",
            icon: app?.icon ?? NSImage(),
            shortTitle: title,
            fullTitle: title,
            isDimmed: false,
            target: .window(window)
        )
    }

    private static func appItem(_ app: NSRunningApplication, windows: [WindowInfo]) -> RingItem {
        let name = app.localizedName ?? "?"
        let frontWindow = windows.first { !$0.isMinimized } ?? windows.first
        let count = windows.count
        let detail = count == 0 ? "no windows" : count == 1 ? "1 window" : "\(count) windows"
        return RingItem(
            id: "a\(app.processIdentifier)",
            icon: app.icon ?? NSImage(),
            shortTitle: name,
            fullTitle: "\(name) — \(detail)",
            isDimmed: count == 0,
            target: .app(app, frontWindow: frontWindow),
            windows: count >= 2 ? windows.prefix(RingGeometry.maxInnerItems).map { windowItem($0, app: app) } : []
        )
    }

    private static func sortedByRecency(_ apps: [NSRunningApplication]) -> [NSRunningApplication] {
        var rank: [pid_t: Int] = [:]
        let info = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
        for window in info where (window[kCGWindowLayer as String] as? Int) == 0 {
            if let pid = window[kCGWindowOwnerPID as String] as? pid_t, rank[pid] == nil {
                rank[pid] = rank.count
            }
        }
        return apps.enumerated()
            .sorted { a, b in
                let ra = rank[a.element.processIdentifier] ?? Int.max
                let rb = rank[b.element.processIdentifier] ?? Int.max
                return ra != rb ? ra < rb : a.offset < b.offset
            }
            .map(\.element)
    }
}
