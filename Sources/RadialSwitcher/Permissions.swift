import ApplicationServices
import CoreGraphics

enum Permissions {
    static func accessibilityGranted(prompt: Bool) -> Bool {
        let key = "AXTrustedCheckOptionPrompt"
        return AXIsProcessTrustedWithOptions([key: prompt] as CFDictionary)
    }

    static func screenRecordingGranted(prompt: Bool) -> Bool {
        if CGPreflightScreenCaptureAccess() { return true }
        if prompt { return CGRequestScreenCaptureAccess() }
        return false
    }
}
