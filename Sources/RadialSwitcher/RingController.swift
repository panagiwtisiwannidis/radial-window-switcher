import AppKit
import Carbon
import ScreenCaptureKit
import SwiftUI

@MainActor
final class RingController {
    private let model = RingModel()
    private let livePreview = LivePreview()
    private lazy var store = ThumbnailStore(model: model)
    private var lastLiveFrame: (windowID: CGWindowID, image: CGImage)?
    private var liveWindowTitle = ""
    private var snapshotTimer: Timer?
    private var isSnapshotting = false
    private static let snapshotInterval: TimeInterval = 5
    private var pendingLiveStart: Task<Void, Never>?
    private static let liveStartDelay: Duration = .milliseconds(80)
    private var pendingTabsDwell: Task<Void, Never>?
    private static let tabsStillness: TimeInterval = 0.15
    private var lastMouseMoveTime = Date()
    private var openMouseLocation: CGPoint = .zero
    private var hasMovedSinceOpen = false
    private static let moveThreshold: CGFloat = 4
    private var openFrontmost: NSRunningApplication?
    private var pendingReload: Task<Void, Never>?

    init() {
        model.onSelectionChange = { [weak self] in self?.selectionChanged() }
        livePreview.onFrame = { [weak self] id, image in
            self?.lastLiveFrame = (id, image)
            self?.model.liveFrame = (id, NSImage(cgImage: image, size: .zero))
        }
    }
    private var panel: RingPanel?
    private var mouseTimer: Timer?
    private var sessionKeys: [HotKey] = []
    private var lastMouseLocation: CGPoint?
    private var screenCenter: CGPoint = .zero
    private var isOpening = false
    private var isVisible: Bool { panel?.isVisible ?? false }
    private var cachedWindows: [WindowInfo]?

    func prewarm() async {
        await store.loadFromDisk()
        let windows = await listWindows()
        cachedWindows = windows
        await refreshTabs(windows)
        await snapshotVisibleWindows()
        snapshotTimer = Timer.scheduledTimer(withTimeInterval: Self.snapshotInterval, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self else { return }
                Task { await self.snapshotVisibleWindows() }
            }
        }
    }

    private func snapshotVisibleWindows() async {
        guard !isVisible, !isSnapshotting, Permissions.screenRecordingGranted(prompt: false) else { return }
        isSnapshotting = true
        defer { isSnapshotting = false }

        guard let content = try? await SCShareableContent.excludingDesktopWindows(true, onScreenWindowsOnly: true) else { return }
        let appPIDs = Set(NSWorkspace.shared.runningApplications.filter { $0.activationPolicy == .regular }.map(\.processIdentifier))
        let windows = content.windows.filter { window in
            guard let pid = window.owningApplication?.processID else { return false }
            return window.windowLayer == 0 && appPIDs.contains(pid) && pid != getpid()
                && window.frame.width > 100 && window.frame.height > 100
        }
        let titles = Dictionary(windows.map { ($0.windowID, $0.title ?? "") }, uniquingKeysWith: { a, _ in a })
        let images = await Thumbnailer.capture(windows.map(CaptureTarget.init))
        for (id, image) in images {
            store.record(image, windowID: id, title: titles[id] ?? "")
        }
    }

    func open() async {
        guard !isVisible, !isOpening else { return }
        let frontmost = NSWorkspace.shared.frontmostApplication
        openFrontmost = frontmost

        if let cachedWindows {
            fill(windows: cachedWindows, frontmost: frontmost)
            show()
        } else {
            isOpening = true
            let windows = await listWindows()
            isOpening = false
            fill(windows: windows, frontmost: frontmost)
            show()
        }

        let fresh = await listWindows()
        cachedWindows = fresh
        if isVisible { fill(windows: fresh, frontmost: frontmost) }
        await refreshTabs(fresh)
        await refreshThumbnails(liveWindowIDs: Set(fresh.compactMap(\.windowID)))
    }

    private func refreshTabs(_ windows: [WindowInfo]) async {
        let started = Date()
        let tabs = await Task.detached { TabReader.tabs(of: windows) }.value
        print(String(format: "Read tabs of %d windows in %.0f ms", tabs.count, Date().timeIntervalSince(started) * 1000))
        model.tabsByWindow = tabs
        if let index = model.innerTabsWindowIndex, !model.innerWindowHasTabs(index) {
            model.innerTabsWindowIndex = nil
        }
        if model.targetItem == nil { model.selection = nil }
    }

    private func refreshThumbnails(liveWindowIDs: Set<CGWindowID>) async {
        guard Permissions.screenRecordingGranted(prompt: false) else { return }
        let items = model.inner + model.outer + model.outer.flatMap(\.windows)
        let targets = items.compactMap(\.previewWindow)
        let started = Date()
        let images = await Thumbnailer.capture(targets)
        print(String(format: "Captured %d/%d thumbnails in %.0f ms", images.count, targets.count, Date().timeIntervalSince(started) * 1000))

        store.prune(liveWindowIDs: liveWindowIDs)
        for window in targets {
            if let id = window.windowID, let image = images[id] {
                store.record(image, windowID: id, title: window.title)
            }
        }
    }

    private func listWindows() async -> [WindowInfo] {
        let started = Date()
        let windows = (try? await WindowEnumerator.listWindows(useScreenCaptureKit: Permissions.screenRecordingGranted(prompt: false))) ?? []
        print(String(format: "Listed %d windows in %.0f ms", windows.count, Date().timeIntervalSince(started) * 1000))
        return windows
    }

    private func fill(windows: [WindowInfo], frontmost: NSRunningApplication?) {
        let content = RingContentBuilder.build(windows: windows, frontmost: frontmost)
        model.inner = content.inner
        model.outer = content.outer
        if let index = model.innerTabsWindowIndex, !model.innerWindowHasTabs(index) {
            model.innerTabsWindowIndex = nil
        }
        if model.targetItem == nil { model.selection = nil }
    }

    func hotkeyReleased() {
        guard isVisible, model.selectedItem != nil else { return }
        confirm()
    }

    private func show() {
        let mouse = NSEvent.mouseLocation
        let screen = NSScreen.screens.first { NSMouseInRect(mouse, $0.frame, false) } ?? NSScreen.main
        guard let screen else { return }

        let radius = RingGeometry.outerRadii.upperBound + 44
        let frame = screen.frame
        let visible = screen.visibleFrame
        func clamp(_ value: CGFloat, _ low: CGFloat, _ high: CGFloat, fallback: CGFloat) -> CGFloat {
            low <= high ? min(max(value, low), high) : fallback
        }
        screenCenter = CGPoint(
            x: clamp(mouse.x, visible.minX + radius, visible.maxX - radius, fallback: visible.midX),
            y: clamp(mouse.y, visible.minY + radius, visible.maxY - radius, fallback: visible.midY)
        )
        model.center = CGPoint(x: screenCenter.x - frame.minX, y: frame.maxY - screenCenter.y)
        let edgeMargin: CGFloat = 8
        RingGeometry.visibleArea = visible
            .insetBy(dx: edgeMargin, dy: edgeMargin)
            .offsetBy(dx: -screenCenter.x, dy: -screenCenter.y)
        model.previewOffsetX = previewOffsetX(in: frame)

        let panel = self.panel ?? makePanel()
        self.panel = panel
        panel.setFrame(frame, display: true)
        model.isShown = false
        panel.orderFrontRegardless()
        DispatchQueue.main.async { [model] in
            withAnimation(RingAnimation.appear) { model.isShown = true }
        }

        lastMouseLocation = NSEvent.mouseLocation
        openMouseLocation = NSEvent.mouseLocation
        hasMovedSinceOpen = false
        mouseTimer = Timer.scheduledTimer(withTimeInterval: 1.0 / 120, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.mouseTick() }
        }
        registerSessionKeys()
    }

    private func registerSessionKeys() {
        func key(_ code: Int, _ modifiers: Int, _ action: @escaping @MainActor () -> Void) {
            guard let hotKey = HotKey(keyCode: code, modifiers: modifiers) else { return }
            hotKey.onPress = action
            sessionKeys.append(hotKey)
        }
        key(kVK_Escape, 0) { [weak self] in self?.close() }
        key(kVK_Return, 0) { [weak self] in self?.confirm() }
        for modifiers in [0, optionKey] {
            key(kVK_Tab, modifiers) { [weak self] in self?.model.moveSelection(forward: true) }
            key(kVK_Tab, modifiers | shiftKey) { [weak self] in self?.model.moveSelection(forward: false) }
        }
    }

    private func previewOffsetX(in frame: CGRect) -> CGFloat {
        let distance = PreviewLayout.distanceFromCenter
        let halfCard = PreviewLayout.cardWidth / 2
        let margin: CGFloat = 8
        if screenCenter.x + distance + halfCard <= frame.maxX - margin { return distance }
        if screenCenter.x - distance - halfCard >= frame.minX + margin { return -distance }
        if frame.maxX - screenCenter.x >= screenCenter.x - frame.minX {
            return frame.maxX - margin - halfCard - screenCenter.x
        }
        return frame.minX + margin + halfCard - screenCenter.x
    }

    private func makePanel() -> RingPanel {
        let panel = RingPanel()
        let hosting = RingHostingView(rootView: RingOverlayView(model: model))
        hosting.onMouseDown = { [weak self] in self?.clicked() }
        hosting.onRightMouseDown = { [weak self] in self?.close() }
        panel.contentView = hosting
        return panel
    }

    private func mouseTick() {
        let mouse = NSEvent.mouseLocation
        guard mouse != lastMouseLocation else { return }
        lastMouseLocation = mouse
        lastMouseMoveTime = Date()
        if !hasMovedSinceOpen {
            guard hypot(mouse.x - openMouseLocation.x, mouse.y - openMouseLocation.y) >= Self.moveThreshold else { return }
            hasMovedSinceOpen = true
        }
        updateSelectionFromMouse()
        let hovered = isOverCloseButton
        if hovered != model.isCloseHovered { model.isCloseHovered = hovered }
    }

    private var isOverCloseButton: Bool {
        guard let segment = model.selectedSegment else { return false }
        let button = RingGeometry.closeButtonOffset(bounds: segment.bounds, radii: segment.radii, isFan: segment.isFan)
        let mouse = NSEvent.mouseLocation
        let x = mouse.x - screenCenter.x, y = screenCenter.y - mouse.y
        return hypot(x - button.width, y - button.height) <= RingGeometry.closeButtonHitRadius
    }

    private func updateSelectionFromMouse() {
        let mouse = NSEvent.mouseLocation
        let dx = mouse.x - screenCenter.x, dy = mouse.y - screenCenter.y
        let selection: RingSelection?
        if let tabsWindow = model.innerTabsWindowIndex {
            let tabSelection = RingGeometry.tabPopupSelection(
                dx: dx, dy: dy, innerCount: model.inner.count, innerIndex: tabsWindow, tabCount: model.outerItems.count
            )
            let distance = hypot(dx, dy)
            let ringBoundary = (RingGeometry.innerRadii.upperBound + RingGeometry.outerRadii.lowerBound) / 2
            if tabSelection == nil, distance > ringBoundary, distance <= RingGeometry.outerRadii.upperBound {
                model.innerTabsWindowIndex = nil
                selection = normalSelection(dx: dx, dy: dy, current: nil)
            } else {
                selection = tabSelection
            }
        } else {
            selection = normalSelection(dx: dx, dy: dy, current: model.selection)
        }
        if selection != model.selection { model.selection = selection }
    }

    private func normalSelection(dx: CGFloat, dy: CGFloat, current: RingSelection?) -> RingSelection? {
        RingGeometry.selection(
            dx: dx, dy: dy,
            innerCount: model.inner.count, outerWindowCounts: model.outer.map(\.windows.count),
            tabCount: { [model] app, window in model.tabItems(appIndex: app, windowIndex: window).count },
            current: current
        )
    }

    private func clicked() {
        if hasMovedSinceOpen { updateSelectionFromMouse() }
        if let item = model.selectedItem, isOverCloseButton {
            closeTarget(item)
        } else if model.selectedItem != nil {
            confirm()
        } else {
            close()
        }
    }

    private func closeTarget(_ item: RingItem) {
        switch item.target {
        case .window(let window):
            WindowActivator.close(window)
        case .app(let app, _):
            app.terminate()
        case .tab(let tab, let window):
            if TabReader.close(tab), let id = window.windowID, var tabs = model.tabsByWindow[id] {
                tabs.removeAll { CFEqual($0.element, tab.element) }
                model.tabsByWindow[id] = tabs
            }
        }
        model.selection = nil
        model.isCloseHovered = false
        stopLivePreview()
        openMouseLocation = NSEvent.mouseLocation
        hasMovedSinceOpen = false
        pendingReload?.cancel()
        pendingReload = Task { [weak self] in
            for delay in [250, 1000] {
                try? await Task.sleep(for: .milliseconds(delay))
                guard !Task.isCancelled, let self, self.isVisible else { return }
                await self.reload()
            }
        }
    }

    private func reload() async {
        let fresh = await listWindows()
        cachedWindows = fresh
        guard isVisible else { return }
        fill(windows: fresh, frontmost: openFrontmost)
        await refreshTabs(fresh)
    }

    private func confirm() {
        let item = model.selectedItem
        close()
        guard let item else { return }

        switch item.target {
        case .window(let window):
            WindowActivator.activate(window)
        case .app(_, frontWindow: .some(let window)):
            WindowActivator.activate(window)
        case .tab(let tab, let window):
            TabReader.select(tab)
            WindowActivator.activate(window)
        case .app(let app, frontWindow: nil):
            if let url = app.bundleURL {
                NSWorkspace.shared.openApplication(at: url, configuration: NSWorkspace.OpenConfiguration())
            } else {
                app.activate()
            }
        }
    }

    private func selectionChanged() {
        updateTabMode()
        pendingLiveStart?.cancel()
        pendingLiveStart = nil

        let item = model.selectedItem
        var window = item?.previewWindow
        if case .tab(let tab, _) = item?.target, !tab.isActive { window = nil }

        guard isVisible, let window, window.windowID != livePreview.windowID else {
            if window == nil { stopLivePreview() }
            return
        }
        pendingLiveStart = Task { [weak self] in
            try? await Task.sleep(for: Self.liveStartDelay)
            guard !Task.isCancelled, let self else { return }
            self.stopLivePreview()
            self.liveWindowTitle = window.title
            self.livePreview.start(window)
        }
    }

    private func updateTabMode() {
        let selection = model.selection
        if let tabsWindow = model.innerTabsWindowIndex {
            let onItsTabs = selection?.level == .outer
            let onItsWindow = selection?.level == .inner && selection?.index == tabsWindow
            if onItsTabs || onItsWindow || (selection == nil && !isCursorInDeadZone) { return }
            model.innerTabsWindowIndex = nil
        }

        pendingTabsDwell?.cancel()
        pendingTabsDwell = nil
        guard let selection, selection.level == .inner, model.innerWindowHasTabs(selection.index) else { return }
        pendingTabsDwell = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .milliseconds(30))
                guard !Task.isCancelled, let self, self.isVisible, self.model.selection == selection else { return }
                if Date().timeIntervalSince(self.lastMouseMoveTime) >= Self.tabsStillness {
                    self.model.innerTabsWindowIndex = selection.index
                    return
                }
            }
        }
    }

    private var isCursorInDeadZone: Bool {
        let mouse = NSEvent.mouseLocation
        return hypot(mouse.x - screenCenter.x, mouse.y - screenCenter.y) <= RingGeometry.deadZoneRadius
    }

    private func stopLivePreview() {
        livePreview.stop()
        if let frame = lastLiveFrame { store.record(frame.image, windowID: frame.windowID, title: liveWindowTitle) }
        lastLiveFrame = nil
        model.liveFrame = nil
    }

    private func close() {
        pendingLiveStart?.cancel()
        pendingLiveStart = nil
        pendingReload?.cancel()
        pendingReload = nil
        model.isCloseHovered = false
        pendingTabsDwell?.cancel()
        pendingTabsDwell = nil
        model.innerTabsWindowIndex = nil
        stopLivePreview()
        mouseTimer?.invalidate()
        mouseTimer = nil
        sessionKeys.forEach { $0.unregister() }
        sessionKeys = []
        lastMouseLocation = nil
        panel?.orderOut(nil)
        model.isShown = false
        model.selection = nil
    }
}

final class RingPanel: NSPanel {
    init() {
        super.init(contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        isOpaque = false
        backgroundColor = .clear
        hasShadow = false
        level = .popUpMenu
        hidesOnDeactivate = false
        collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary, .ignoresCycle]
    }

    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
}

final class RingHostingView: NSHostingView<RingOverlayView> {
    var onMouseDown: (() -> Void)?
    var onRightMouseDown: (() -> Void)?

    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    override func mouseDown(with event: NSEvent) { onMouseDown?() }
    override func rightMouseDown(with event: NSEvent) { onRightMouseDown?() }
}
