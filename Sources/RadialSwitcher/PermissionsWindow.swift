import AppKit
import SwiftUI

@MainActor
final class PermissionsState: ObservableObject {
    @Published private(set) var accessibility = false
    @Published private(set) var screenRecording = false
    @Published private(set) var needsRestart = false

    private let screenRecordingAtLaunch: Bool
    private var timer: Timer?

    var allGranted: Bool { accessibility && screenRecording && !needsRestart }

    init() {
        screenRecordingAtLaunch = Permissions.screenRecordingGranted(prompt: false)
        refresh()
    }

    func startPolling() {
        timer?.invalidate()
        timer = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.refresh() }
        }
    }

    func stopPolling() {
        timer?.invalidate()
        timer = nil
    }

    func refresh() {
        accessibility = Permissions.accessibilityGranted(prompt: false)
        screenRecording = Permissions.screenRecordingGranted(prompt: false)
        needsRestart = screenRecording && !screenRecordingAtLaunch
    }

    func requestAccessibility() {
        _ = Permissions.accessibilityGranted(prompt: true)
        open("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
    }

    func requestScreenRecording() {
        _ = Permissions.screenRecordingGranted(prompt: true)
        open("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
    }

    func restartApp() {
        let configuration = NSWorkspace.OpenConfiguration()
        configuration.createsNewApplicationInstance = true
        NSWorkspace.shared.openApplication(at: Bundle.main.bundleURL, configuration: configuration) { _, _ in
            DispatchQueue.main.async { NSApp.terminate(nil) }
        }
    }

    private func open(_ url: String) {
        if let url = URL(string: url) { NSWorkspace.shared.open(url) }
    }
}

@MainActor
final class PermissionsWindowController {
    private let state = PermissionsState()
    private var window: NSWindow?

    var allGranted: Bool {
        state.refresh()
        return state.allGranted
    }

    func show() {
        state.refresh()
        state.startPolling()
        let window = self.window ?? makeWindow()
        self.window = window
        window.level = .floating
        window.center()
        NSApp.activate(ignoringOtherApps: true)
        window.makeKeyAndOrderFront(nil)
        window.orderFrontRegardless()
    }

    private func makeWindow() -> NSWindow {
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 480, height: 360),
            styleMask: [.titled, .closable],
            backing: .buffered,
            defer: false
        )
        window.title = "Radial Switcher"
        window.isReleasedWhenClosed = false
        window.contentView = NSHostingView(rootView: PermissionsView(state: state) { [weak self] in
            self?.window?.close()
        })
        NotificationCenter.default.addObserver(forName: NSWindow.willCloseNotification, object: window, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.state.stopPolling() }
        }
        return window
    }
}

private struct PermissionsView: View {
    @ObservedObject var state: PermissionsState
    let close: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            Text("Radial Switcher needs two permissions")
                .font(.title2.weight(.semibold))

            PermissionRow(
                title: "Accessibility",
                detail: "To list your windows and tabs and bring the one you pick to the front.",
                granted: state.accessibility,
                action: state.requestAccessibility
            )
            PermissionRow(
                title: "Screen & System Audio Recording",
                detail: "To show window previews. Nothing is recorded or saved outside your Mac.",
                granted: state.screenRecording,
                action: state.requestScreenRecording
            )

            Spacer(minLength: 0)

            HStack {
                if state.needsRestart {
                    Text("Restart Radial Switcher so screen recording takes effect.")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                    Spacer()
                    Button("Restart", action: state.restartApp)
                        .keyboardShortcut(.defaultAction)
                } else {
                    Text("Hold ⌥Space anywhere to open the ring.")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                    Spacer()
                    Button(state.allGranted ? "Done" : "Later", action: close)
                        .keyboardShortcut(.defaultAction)
                }
            }
        }
        .padding(24)
        .frame(width: 480, height: 360)
    }
}

private struct PermissionRow: View {
    let title: String
    let detail: String
    let granted: Bool
    let action: () -> Void

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: granted ? "checkmark.circle.fill" : "exclamationmark.circle")
                .font(.title2)
                .foregroundStyle(granted ? Color.green : Color.orange)
            VStack(alignment: .leading, spacing: 4) {
                Text(title).font(.headline)
                Text(detail)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer()
            if !granted {
                Button("Open Settings", action: action)
            }
        }
    }
}
