import AppKit
import Carbon

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private let ring = RingController()
    private let permissions = PermissionsWindowController()
    private var statusItem: StatusItemController?
    private var hotKey: HotKey?

    func applicationDidFinishLaunching(_ notification: Notification) {
        statusItem = StatusItemController { [permissions] in permissions.show() }

        guard let hotKey = HotKey(keyCode: kVK_Space, modifiers: optionKey) else {
            let alert = NSAlert()
            alert.messageText = "⌥Space is already in use"
            alert.informativeText = "Another app has taken the ⌥Space shortcut. Quit it (or change its shortcut) and open Radial Switcher again."
            NSApp.activate()
            alert.runModal()
            NSApp.terminate(nil)
            return
        }
        hotKey.onPress = { [ring] in Task { await ring.open() } }
        hotKey.onRelease = { [ring] in ring.hotkeyReleased() }
        self.hotKey = hotKey

        if !permissions.allGranted {
            permissions.show()
        }

        Task { [ring] in await ring.prewarm() }
        print("Ready. Hold ⌥Space, aim with the mouse, release (or click). Esc cancels.")
    }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        permissions.show()
        return false
    }
}
