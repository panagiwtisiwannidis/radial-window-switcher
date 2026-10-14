import AppKit

@MainActor
enum DebugCLI {
    static func run(_ args: [String]) async -> Int32 {
        let hasAX = Permissions.accessibilityGranted(prompt: true)
        let hasScreen = Permissions.screenRecordingGranted(prompt: true)
        print("Accessibility: \(hasAX ? "OK" : "MISSING")   Screen Recording: \(hasScreen ? "OK" : "MISSING")")
        if !hasAX {
            print("Grant Accessibility to your terminal app in System Settings → Privacy & Security, then run again.")
            return 1
        }
        if !hasScreen {
            print("Screen Recording missing: listing Accessibility windows only (no capture check, no other Spaces).")
        }

        let windows: [WindowInfo]
        do {
            windows = try await WindowEnumerator.listWindows(useScreenCaptureKit: hasScreen)
        } catch {
            print("ScreenCaptureKit error: \(error)")
            return 1
        }

        switch args.first {
        case "list":
            printWindows(windows, hasScreen: hasScreen)
            return 0
        case "focus" where args.count == 2:
            guard let id = CGWindowID(args[1]), let target = windows.first(where: { $0.windowID == id }) else {
                print("No window with id \(args[1])")
                return 1
            }
            print("Focusing [\(id)] \(target.appName) — \(target.title)")
            WindowActivator.activate(target)
            return 0
        default:
            print("Usage: RadialSwitcher [list | focus <windowID>]   (no arguments: run the switcher)")
            return 1
        }
    }

    private static func printWindows(_ windows: [WindowInfo], hasScreen: Bool) {
        let frontPID = NSWorkspace.shared.frontmostApplication?.processIdentifier
        let grouped = Dictionary(grouping: windows, by: \.pid)
        let pids = grouped.keys.sorted { a, b in
            if a == frontPID { return true }
            if b == frontPID { return false }
            return grouped[a]![0].appName < grouped[b]![0].appName
        }

        for pid in pids {
            let appWindows = grouped[pid]!
            let marker = pid == frontPID ? "  ← frontmost" : ""
            print("\n\(appWindows[0].appName) (pid \(pid), \(appWindows[0].bundleID ?? "-"))\(marker)")
            for w in appWindows {
                var flags: [String] = []
                if w.isMinimized { flags.append("minimized") }
                if hasScreen && !w.isCapturable { flags.append("no-capture") }
                if w.isOnOtherSpace { flags.append("other Space") }
                let id = w.windowID.map(String.init) ?? "?"
                let frame = w.frame.map { "\(Int($0.width))x\(Int($0.height))@\(Int($0.minX)),\(Int($0.minY))" } ?? "-"
                let title = w.title.isEmpty ? "(untitled)" : w.title
                print("  [\(id)] \(title)  \(frame)  \(flags.isEmpty ? "" : "{\(flags.joined(separator: ", "))}")")
            }
        }
        print("\nTotal: \(windows.count) windows in \(pids.count) apps")
    }
}
