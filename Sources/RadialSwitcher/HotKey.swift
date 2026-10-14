import Carbon

@MainActor
final class HotKey {
    var onPress: (() -> Void)?
    var onRelease: (() -> Void)?

    private var ref: EventHotKeyRef?
    private let id: UInt32

    private static var registry: [UInt32: HotKey] = [:]
    private static var nextID: UInt32 = 1
    private static var handlerInstalled = false
    private static let signature: OSType = 0x5253_5754

    init?(keyCode: Int, modifiers: Int) {
        id = HotKey.nextID
        HotKey.nextID += 1
        HotKey.installHandlerIfNeeded()

        let hotKeyID = EventHotKeyID(signature: HotKey.signature, id: id)
        let status = RegisterEventHotKey(UInt32(keyCode), UInt32(modifiers), hotKeyID, GetApplicationEventTarget(), 0, &ref)
        guard status == noErr else { return nil }
        HotKey.registry[id] = self
    }

    func unregister() {
        if let ref { UnregisterEventHotKey(ref) }
        ref = nil
        HotKey.registry[id] = nil
    }

    private static func installHandlerIfNeeded() {
        guard !handlerInstalled else { return }
        handlerInstalled = true

        var eventTypes = [
            EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed)),
            EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyReleased)),
        ]
        InstallEventHandler(GetApplicationEventTarget(), hotKeyEventHandler, eventTypes.count, &eventTypes, nil, nil)
    }

    fileprivate static func dispatch(id: UInt32, pressed: Bool) {
        guard let hotKey = registry[id] else { return }
        if pressed { hotKey.onPress?() } else { hotKey.onRelease?() }
    }
}

private func hotKeyEventHandler(_: EventHandlerCallRef?, _ event: EventRef?, _: UnsafeMutableRawPointer?) -> OSStatus {
    guard let event else { return OSStatus(eventNotHandledErr) }
    var hotKeyID = EventHotKeyID()
    let status = GetEventParameter(
        event, EventParamName(kEventParamDirectObject), EventParamType(typeEventHotKeyID),
        nil, MemoryLayout<EventHotKeyID>.size, nil, &hotKeyID
    )
    guard status == noErr else { return status }
    let pressed = GetEventKind(event) == UInt32(kEventHotKeyPressed)
    MainActor.assumeIsolated {
        HotKey.dispatch(id: hotKeyID.id, pressed: pressed)
    }
    return noErr
}
