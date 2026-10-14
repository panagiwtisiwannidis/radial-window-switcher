import AppKit
import ApplicationServices

struct DemoContent {
    var inner: [RingItem]
    var outer: [RingItem]
    var tabs: [CGWindowID: [TabInfo]]
    var thumbnails: [CGWindowID: NSImage]
    var tabThumbnails: [String: NSImage]
    var selection: RingSelection
    var tabsWindowIndex: Int?
}

@MainActor
final class DemoDelegate: NSObject, NSApplicationDelegate {
    private let ring = RingController()
    private let scene: String

    init(scene: String) {
        self.scene = scene
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        guard let content = Demo.content(scene: scene) else {
            print("Usage: RadialSwitcher demo [windows | tabs | apps]")
            NSApp.terminate(nil)
            return
        }
        ring.presentDemo(content)
        print("Showing the \(scene) demo. Esc closes it.")
    }
}

@MainActor
enum Demo {
    private struct App {
        let name: String
        let bundleID: String
        let windows: [String]
    }

    private static let front = App(name: "Safari", bundleID: "com.apple.Safari",
                                   windows: ["Trip to Lisbon", "Weekend recipes", "Swift documentation"])
    private static let frontTabs = ["Trip to Lisbon", "Flights to LIS", "Hotels in Alfama", "Weather in Lisbon", "Tram 28 guide"]

    private static let others = [
        App(name: "Notes", bundleID: "com.apple.Notes", windows: ["Groceries", "Book list", "Meeting notes"]),
        App(name: "Mail", bundleID: "com.apple.mail", windows: ["Inbox"]),
        App(name: "Music", bundleID: "com.apple.Music", windows: ["Music"]),
        App(name: "Maps", bundleID: "com.apple.Maps", windows: ["Lisbon"]),
        App(name: "Photos", bundleID: "com.apple.Photos", windows: ["Library"]),
        App(name: "Calendar", bundleID: "com.apple.iCal", windows: ["Calendar"]),
        App(name: "Messages", bundleID: "com.apple.MobileSMS", windows: ["Messages"]),
        App(name: "Finder", bundleID: "com.apple.finder", windows: ["Documents", "Downloads"]),
        App(name: "System Settings", bundleID: "com.apple.systempreferences", windows: ["Appearance"]),
    ]

    private static let palettes: [(NSColor, NSColor)] = [
        (NSColor(red: 0.20, green: 0.47, blue: 0.95, alpha: 1), NSColor(red: 0.93, green: 0.95, blue: 0.99, alpha: 1)),
        (NSColor(red: 0.98, green: 0.62, blue: 0.20, alpha: 1), NSColor(red: 0.99, green: 0.96, blue: 0.91, alpha: 1)),
        (NSColor(red: 0.30, green: 0.75, blue: 0.45, alpha: 1), NSColor(red: 0.93, green: 0.98, blue: 0.94, alpha: 1)),
        (NSColor(red: 0.85, green: 0.30, blue: 0.45, alpha: 1), NSColor(red: 0.99, green: 0.93, blue: 0.95, alpha: 1)),
        (NSColor(red: 0.55, green: 0.40, blue: 0.90, alpha: 1), NSColor(red: 0.95, green: 0.93, blue: 0.99, alpha: 1)),
        (NSColor(red: 0.15, green: 0.65, blue: 0.75, alpha: 1), NSColor(red: 0.92, green: 0.97, blue: 0.98, alpha: 1)),
    ]

    static func content(scene: String) -> DemoContent? {
        var nextID: CGWindowID = 900_000
        var thumbnails: [CGWindowID: NSImage] = [:]
        var tabThumbnails: [String: NSImage] = [:]
        var tabs: [CGWindowID: [TabInfo]] = [:]
        let placeholder = AXUIElementCreateApplication(getpid())

        func windows(of app: App, pid: pid_t) -> [WindowInfo] {
            app.windows.map { title in
                nextID += 1
                thumbnails[nextID] = mockWindow(seed: Int(nextID))
                return WindowInfo(appName: app.name, pid: pid, bundleID: nil, windowID: nextID, title: title,
                                  isMinimized: false, frame: nil, scWindow: nil, axElement: nil, isOnOtherSpace: false)
            }
        }

        func icon(_ app: App) -> NSImage {
            NSWorkspace.shared.urlForApplication(withBundleIdentifier: app.bundleID)
                .map { NSWorkspace.shared.icon(forFile: $0.resolvingSymlinksInPath().path) } ?? NSImage()
        }

        func windowItem(_ window: WindowInfo, icon: NSImage) -> RingItem {
            RingItem(id: "w\(window.windowID ?? 0)", icon: icon, shortTitle: window.title, fullTitle: window.title,
                     isDimmed: false, target: .window(window))
        }

        let frontWindows = windows(of: front, pid: 1000)
        let frontIcon = icon(front)
        if let first = frontWindows.first?.windowID {
            tabs[first] = frontTabs.enumerated().map { i, title in
                TabInfo(title: title, isActive: i == 0, element: placeholder)
            }
            for (i, title) in frontTabs.enumerated() {
                tabThumbnails[ThumbnailStore.tabKey(windowID: first, title: title)] = mockWindow(seed: Int(first) + i * 7)
            }
        }
        let inner = frontWindows.map { windowItem($0, icon: frontIcon) }

        let outer = others.enumerated().map { index, app -> RingItem in
            let appWindows = windows(of: app, pid: pid_t(1001 + index))
            let appIcon = icon(app)
            let count = appWindows.count
            return RingItem(id: "a\(1001 + index)", icon: appIcon, shortTitle: app.name,
                            fullTitle: "\(app.name) — \(count == 1 ? "1 window" : "\(count) windows")",
                            isDimmed: false, target: .app(NSRunningApplication.current, frontWindow: appWindows.first),
                            windows: count >= 2 ? appWindows.map { windowItem($0, icon: appIcon) } : [])
        }

        let selection: RingSelection
        var tabsWindowIndex: Int?
        switch scene {
        case "windows":
            selection = RingSelection(level: .outer, index: 0)
        case "tabs":
            selection = RingSelection(level: .outer, index: 2)
            tabsWindowIndex = 0
        case "apps":
            selection = RingSelection(level: .outer, index: 3)
        default:
            return nil
        }
        return DemoContent(inner: inner, outer: outer, tabs: tabs, thumbnails: thumbnails,
                           tabThumbnails: tabThumbnails, selection: selection, tabsWindowIndex: tabsWindowIndex)
    }

    private static func mockWindow(seed: Int) -> NSImage {
        let (accent, paper) = palettes[seed % palettes.count]
        let size = NSSize(width: 600, height: 400)
        return NSImage(size: size, flipped: true) { _ in
            NSColor.white.setFill()
            NSBezierPath(roundedRect: NSRect(origin: .zero, size: size), xRadius: 14, yRadius: 14).fill()
            NSColor(white: 0.94, alpha: 1).setFill()
            NSRect(x: 0, y: 0, width: size.width, height: 44).fill()
            for (i, color) in [NSColor.systemRed, .systemYellow, .systemGreen].enumerated() {
                color.setFill()
                NSBezierPath(ovalIn: NSRect(x: 18 + CGFloat(i) * 22, y: 15, width: 14, height: 14)).fill()
            }
            paper.setFill()
            NSRect(x: 0, y: 44, width: 150, height: size.height - 44).fill()
            accent.withAlphaComponent(0.35).setFill()
            for row in 0..<5 {
                NSBezierPath(roundedRect: NSRect(x: 18, y: 66 + CGFloat(row) * 34, width: 110 - CGFloat((row + seed) % 3) * 18,
                                                 height: 14), xRadius: 7, yRadius: 7).fill()
            }
            accent.setFill()
            NSBezierPath(roundedRect: NSRect(x: 172, y: 66, width: 404, height: 140), xRadius: 12, yRadius: 12).fill()
            NSColor.white.withAlphaComponent(0.85).setFill()
            NSBezierPath(roundedRect: NSRect(x: 196, y: 92, width: 180 + CGFloat(seed % 4) * 30, height: 18), xRadius: 9, yRadius: 9).fill()
            NSBezierPath(roundedRect: NSRect(x: 196, y: 122, width: 120, height: 12), xRadius: 6, yRadius: 6).fill()
            for column in 0..<3 {
                paper.setFill()
                NSBezierPath(roundedRect: NSRect(x: 172 + CGFloat(column) * 138, y: 226, width: 128, height: 150),
                             xRadius: 10, yRadius: 10).fill()
                accent.withAlphaComponent(0.55).setFill()
                NSBezierPath(roundedRect: NSRect(x: 184 + CGFloat(column) * 138, y: 240, width: 104, height: 70),
                             xRadius: 8, yRadius: 8).fill()
                NSColor(white: 0.75, alpha: 1).setFill()
                NSBezierPath(roundedRect: NSRect(x: 184 + CGFloat(column) * 138, y: 324, width: 90, height: 10),
                             xRadius: 5, yRadius: 5).fill()
                NSBezierPath(roundedRect: NSRect(x: 184 + CGFloat(column) * 138, y: 344, width: 64, height: 10),
                             xRadius: 5, yRadius: 5).fill()
            }
            return true
        }
    }
}
