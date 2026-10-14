import AppKit
import Carbon

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private let ring = RingController()
    private var hotKey: HotKey?

    func applicationDidFinishLaunching(_ notification: Notification) {
        let hasAX = Permissions.accessibilityGranted(prompt: true)
        let hasScreen = Permissions.screenRecordingGranted(prompt: true)
        print("Accessibility: \(hasAX ? "OK" : "MISSING")   Screen Recording: \(hasScreen ? "OK" : "MISSING")")

        guard let hotKey = HotKey(keyCode: kVK_Space, modifiers: optionKey) else {
            print("⌥Space is already taken by another app. Quit it or change the hotkey in AppDelegate.swift.")
            NSApp.terminate(nil)
            return
        }
        hotKey.onPress = { [ring] in Task { await ring.open() } }
        hotKey.onRelease = { [ring] in ring.hotkeyReleased() }
        self.hotKey = hotKey

        Task { [ring] in await ring.prewarm() }
        print("Ready. Hold ⌥Space, aim with the mouse, release (or click). Esc cancels. Ctrl+C quits.")
    }
}
