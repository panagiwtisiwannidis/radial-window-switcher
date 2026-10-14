import AppKit
import ServiceManagement

@MainActor
final class StatusItemController: NSObject, NSMenuDelegate {
    private let statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
    private let showPermissions: () -> Void
    private let loginItem = NSMenuItem(title: "Open at Login", action: #selector(toggleOpenAtLogin), keyEquivalent: "")

    init(showPermissions: @escaping () -> Void) {
        self.showPermissions = showPermissions
        super.init()

        statusItem.button?.image = NSImage(systemSymbolName: "circle.circle", accessibilityDescription: "Radial Switcher")

        let menu = NSMenu()
        menu.delegate = self
        menu.addItem(disabledItem("Radial Switcher"))
        menu.addItem(disabledItem("Hold ⌥Space to switch windows"))
        menu.addItem(.separator())
        menu.addItem(item("Permissions…", #selector(openPermissions)))
        loginItem.target = self
        menu.addItem(loginItem)
        menu.addItem(.separator())
        menu.addItem(item("Quit Radial Switcher", #selector(quit), key: "q"))
        statusItem.menu = menu
    }

    func menuWillOpen(_ menu: NSMenu) {
        loginItem.state = SMAppService.mainApp.status == .enabled ? .on : .off
    }

    @objc private func openPermissions() {
        showPermissions()
    }

    @objc private func toggleOpenAtLogin() {
        do {
            if SMAppService.mainApp.status == .enabled {
                try SMAppService.mainApp.unregister()
            } else {
                try SMAppService.mainApp.register()
            }
        } catch {
            let alert = NSAlert()
            alert.messageText = "Couldn't change Open at Login"
            alert.informativeText = "\(error.localizedDescription)\n\nMove Radial Switcher to the Applications folder and try again."
            NSApp.activate()
            alert.runModal()
        }
    }

    @objc private func quit() {
        NSApp.terminate(nil)
    }

    private func item(_ title: String, _ action: Selector, key: String = "") -> NSMenuItem {
        let item = NSMenuItem(title: title, action: action, keyEquivalent: key)
        item.target = self
        return item
    }

    private func disabledItem(_ title: String) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
        item.isEnabled = false
        return item
    }
}
