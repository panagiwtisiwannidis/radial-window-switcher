import AppKit

struct RingItem: Identifiable {
    enum Target {
        case window(WindowInfo)
        case app(NSRunningApplication, frontWindow: WindowInfo?)
        case tab(TabInfo, window: WindowInfo)
    }

    let id: String
    let icon: NSImage
    let shortTitle: String
    let fullTitle: String
    let isDimmed: Bool
    let target: Target
    var windows: [RingItem] = []

    var previewWindow: WindowInfo? {
        switch target {
        case .window(let window): window
        case .app(_, let frontWindow): frontWindow
        case .tab(_, let window): window
        }
    }
}

enum RingLevel { case inner, outer }

struct RingSelection: Equatable {
    let level: RingLevel
    let index: Int
    var windowIndex: Int? = nil
    var tabIndex: Int? = nil
    var isPointing = true
}

@MainActor
final class LiveFrame: ObservableObject {
    @Published var frame: (windowID: CGWindowID, image: NSImage)?
}

@MainActor
final class RingModel: ObservableObject {
    @Published var inner: [RingItem] = []
    @Published var outer: [RingItem] = []
    @Published var selection: RingSelection? {
        didSet { if selection != oldValue { onSelectionChange?() } }
    }
    var onSelectionChange: (() -> Void)?
    @Published var isShown = false
    @Published var center: CGPoint = .zero
    @Published var previewOffsetX: CGFloat = 0
    @Published var thumbnails: [CGWindowID: NSImage] = [:]
    @Published var tabThumbnails: [String: NSImage] = [:]
    let live = LiveFrame()
    @Published var tabsByWindow: [CGWindowID: [TabInfo]] = [:]
    @Published var innerTabsWindowIndex: Int?
    @Published var isCloseHovered = false

    var outerItems: [RingItem] {
        guard let index = innerTabsWindowIndex else { return outer }
        return innerTabItems(windowIndex: index)
    }

    func innerTabItems(windowIndex: Int) -> [RingItem] {
        guard inner.indices.contains(windowIndex), let window = inner[windowIndex].previewWindow else { return [] }
        return makeTabItems(of: window, icon: inner[windowIndex].icon)
    }

    func innerWindowHasTabs(_ index: Int) -> Bool {
        innerTabItems(windowIndex: index).count >= 2
    }

    var selectedItem: RingItem? {
        selection?.isPointing == true ? targetItem : nil
    }

    var targetItem: RingItem? {
        guard let selection else { return nil }
        let items = selection.level == .inner ? inner : outerItems
        if innerTabsWindowIndex != nil, selection.level == .outer {
            return items.indices.contains(selection.index) ? items[selection.index] : nil
        }
        guard items.indices.contains(selection.index) else { return nil }
        let item = items[selection.index]
        if let tabIndex = selection.tabIndex {
            let tabs = tabItems(appIndex: selection.index, windowIndex: selection.windowIndex)
            return tabs.indices.contains(tabIndex) ? tabs[tabIndex] : nil
        }
        if let windowIndex = selection.windowIndex {
            return item.windows.indices.contains(windowIndex) ? item.windows[windowIndex] : nil
        }
        return item
    }

    var expandedAppIndex: Int? {
        guard innerTabsWindowIndex == nil, let selection, selection.level == .outer,
              outer.indices.contains(selection.index), outer[selection.index].windows.count >= 2
        else { return nil }
        return selection.index
    }

    func tabWindow(appIndex: Int, windowIndex: Int?) -> WindowInfo? {
        guard outer.indices.contains(appIndex) else { return nil }
        let app = outer[appIndex]
        guard app.windows.count >= 2 else { return app.previewWindow }
        guard let windowIndex, app.windows.indices.contains(windowIndex) else { return nil }
        return app.windows[windowIndex].previewWindow
    }

    func tabItems(appIndex: Int, windowIndex: Int?) -> [RingItem] {
        guard let window = tabWindow(appIndex: appIndex, windowIndex: windowIndex) else { return [] }
        return makeTabItems(of: window, icon: outer[appIndex].icon)
    }

    private func makeTabItems(of window: WindowInfo, icon: NSImage) -> [RingItem] {
        guard let id = window.windowID, let tabs = tabsByWindow[id] else { return [] }
        return tabs.enumerated().map { i, tab in
            RingItem(id: "t\(id)-\(i)", icon: icon, shortTitle: tab.title, fullTitle: tab.title,
                     isDimmed: false, target: .tab(tab, window: window))
        }
    }

    var selectedSegment: (bounds: RingGeometry.Bounds, radii: ClosedRange<CGFloat>, isFan: Bool)? {
        guard let selection, selection.isPointing, targetItem != nil else { return nil }
        if selection.level == .inner {
            return (RingGeometry.segmentBounds(index: selection.index, count: inner.count), RingGeometry.innerRadii, false)
        }
        if let tabsWindow = innerTabsWindowIndex {
            let bounds = RingGeometry.tabPopupBounds(innerIndex: tabsWindow, innerCount: inner.count, tabCount: outerItems.count)
            guard bounds.indices.contains(selection.index) else { return nil }
            return (bounds[selection.index], RingGeometry.tabPopupRadii, true)
        }
        let windowCount = outer[selection.index].windows.count
        let windowBounds = windowCount >= 2
            ? RingGeometry.windowArcBounds(appIndex: selection.index, appCount: outer.count, windowCount: windowCount)
            : []
        if let tabIndex = selection.tabIndex {
            guard let arc = RingGeometry.tabArc(
                appIndex: selection.index, appCount: outer.count, windowIndex: selection.windowIndex,
                windowBounds: windowBounds, tabCount: tabItems(appIndex: selection.index, windowIndex: selection.windowIndex).count
            ), arc.bounds.indices.contains(tabIndex) else { return nil }
            return (arc.bounds[tabIndex], arc.radii, true)
        }
        if let windowIndex = selection.windowIndex {
            guard windowBounds.indices.contains(windowIndex) else { return nil }
            return (windowBounds[windowIndex], RingGeometry.windowArcRadii, true)
        }
        return (RingGeometry.segmentBounds(index: selection.index, count: outer.count), RingGeometry.outerRadii, false)
    }

    var aim: RingGeometry.Aim? {
        guard let selection else { return nil }
        if let tabsWindow = innerTabsWindowIndex {
            guard selection.level == .inner, selection.index == tabsWindow else { return nil }
            let bounds = RingGeometry.tabPopupBounds(innerIndex: tabsWindow, innerCount: inner.count, tabCount: outerItems.count)
            guard let first = bounds.first, let last = bounds.last else { return nil }
            return .init(arc: (first.start, last.end), arcInner: RingGeometry.tabPopupRadii.lowerBound)
        }
        guard selection.level == .outer, selection.tabIndex == nil, outer.indices.contains(selection.index) else { return nil }
        let app = selection.index
        let windowCount = outer[app].windows.count
        let windowBounds = windowCount >= 2
            ? RingGeometry.windowArcBounds(appIndex: app, appCount: outer.count, windowCount: windowCount)
            : []
        if selection.windowIndex == nil, windowCount >= 2, let first = windowBounds.first, let last = windowBounds.last {
            return .init(arc: (first.start, last.end), arcInner: RingGeometry.windowArcRadii.lowerBound)
        }
        guard let arc = RingGeometry.tabArc(
            appIndex: app, appCount: outer.count, windowIndex: selection.windowIndex,
            windowBounds: windowBounds, tabCount: tabItems(appIndex: app, windowIndex: selection.windowIndex).count
        ), let first = arc.bounds.first, let last = arc.bounds.last else { return nil }
        return .init(arc: (first.start, last.end), arcInner: arc.radii.lowerBound)
    }

    var openTabArc: (appIndex: Int, windowIndex: Int?, items: [RingItem])? {
        guard innerTabsWindowIndex == nil, let selection, selection.level == .outer else { return nil }
        let items = tabItems(appIndex: selection.index, windowIndex: selection.windowIndex)
        return items.count >= 2 ? (selection.index, selection.windowIndex, items) : nil
    }

    var keyboardOrder: [RingSelection] {
        if innerTabsWindowIndex != nil {
            return inner.indices.map { RingSelection(level: .inner, index: $0) }
                + outerItems.indices.map { RingSelection(level: .outer, index: $0) }
        }
        return inner.indices.map { RingSelection(level: .inner, index: $0) }
            + outer.indices.flatMap { i in
                outer[i].windows.count >= 2
                    ? outer[i].windows.indices.map { RingSelection(level: .outer, index: i, windowIndex: $0) }
                    : [RingSelection(level: .outer, index: i)]
            }
    }

    func moveSelection(forward: Bool) {
        let order = keyboardOrder
        guard !order.isEmpty else { return }
        let position = selection.map { RingSelection(level: $0.level, index: $0.index, windowIndex: $0.windowIndex) }
        if let position, let current = order.firstIndex(of: position) {
            self.selection = order[(current + (forward ? 1 : order.count - 1)) % order.count]
        } else if forward {
            self.selection = order[!inner.isEmpty && order.count > 1 ? 1 : 0]
        } else {
            self.selection = order.last
        }
    }
}

enum RingGeometry {
    static let deadZoneRadius: CGFloat = 45
    static let innerRadii: ClosedRange<CGFloat> = 50...125
    static let outerRadii: ClosedRange<CGFloat> = 132...210
    static let windowArcRadii: ClosedRange<CGFloat> = 216...322
    static let tabArcRadii: ClosedRange<CGFloat> = 328...434
    static let singleWindowTabArcRadii: ClosedRange<CGFloat> = 216...322
    static let tabPopupRadii: ClosedRange<CGFloat> = 214...320
    static let maxRadius = tabArcRadii.upperBound
    static let outsideSlack: CGFloat = 20
    static let maxInnerItems = 8
    private static let windowArcSegment: CGFloat = 0.36
    private static let tabArcSegment: CGFloat = 0.26
    private static let singleWindowTabArcSegment: CGFloat = 0.36
    private static let fanMaxSpan: CGFloat = .pi
    private static let tabPopupSegment: CGFloat = 0.36
    private static let tabPopupMaxSpan: CGFloat = 1.6 * .pi
    private static let tabArcMaxSpan: CGFloat = 1.6 * .pi

    static func tabPopupBounds(innerIndex: Int, innerCount: Int, tabCount: Int) -> [Bounds] {
        let center = CGFloat(innerIndex) * 2 * .pi / CGFloat(innerCount)
        return onScreen(fan(center: center, count: tabCount, segment: tabPopupSegment, maxSpan: tabPopupMaxSpan),
                        outerRadius: tabPopupRadii.upperBound)
    }

    static func tabPopupSelection(
        dx: CGFloat, dy: CGFloat, innerCount: Int, innerIndex: Int, tabCount: Int
    ) -> RingSelection? {
        let distance = hypot(dx, dy)
        guard distance > deadZoneRadius, distance <= tabPopupRadii.upperBound + outsideSlack else { return nil }
        let angle = clockwiseAngleFromTop(dx: dx, dy: dy)
        if distance < (innerRadii.upperBound + outerRadii.lowerBound) / 2 {
            return RingSelection(level: .inner, index: segmentIndex(angle: angle, count: innerCount))
        }
        let bounds = tabPopupBounds(innerIndex: innerIndex, innerCount: innerCount, tabCount: tabCount)
        return fanIndex(angle: angle, bounds: bounds).map { RingSelection(level: .outer, index: $0) }
    }

    static func selection(
        dx: CGFloat, dy: CGFloat,
        innerCount: Int, outerWindowCounts: [Int],
        tabCount: (Int, Int?) -> Int,
        current: RingSelection?
    ) -> RingSelection? {
        let distance = hypot(dx, dy)
        let angle = clockwiseAngleFromTop(dx: dx, dy: dy)
        let outerCount = outerWindowCounts.count

        if let current, current.level == .outer, outerWindowCounts.indices.contains(current.index) {
            let app = current.index
            let windowCount = outerWindowCounts[app]
            let hasWindowArc = windowCount >= 2
            let windowBounds = hasWindowArc ? windowArcBounds(appIndex: app, appCount: outerCount, windowCount: windowCount) : []

            let keepCurrent = RingSelection(level: .outer, index: app, windowIndex: current.windowIndex, isPointing: false)

            if let arc = tabArc(appIndex: app, appCount: outerCount, windowIndex: current.windowIndex,
                                windowBounds: windowBounds, tabCount: tabCount(app, current.windowIndex)),
               distance > arc.innerEdge, distance <= arc.radii.upperBound + outsideSlack
            {
                guard let tabIndex = fanIndex(angle: angle, bounds: arc.bounds) else { return keepCurrent }
                return RingSelection(level: .outer, index: app, windowIndex: current.windowIndex, tabIndex: tabIndex)
            }

            if hasWindowArc, distance > outerRadii.upperBound, distance <= maxRadius + outsideSlack {
                if distance <= windowArcRadii.upperBound, let windowIndex = fanIndex(angle: angle, bounds: windowBounds) {
                    return RingSelection(level: .outer, index: app, windowIndex: windowIndex)
                }
                return keepCurrent
            }
        }

        guard distance > deadZoneRadius, distance <= outerRadii.upperBound + outsideSlack else { return nil }

        let ringBoundary = (innerRadii.upperBound + outerRadii.lowerBound) / 2
        if innerCount > 0, distance < ringBoundary || outerCount == 0 {
            return RingSelection(level: .inner, index: segmentIndex(angle: angle, count: innerCount))
        }
        if outerCount > 0 {
            return RingSelection(level: .outer, index: segmentIndex(angle: angle, count: outerCount))
        }
        return nil
    }

    static func clockwiseAngleFromTop(dx: CGFloat, dy: CGFloat) -> CGFloat {
        let angle = atan2(dx, dy)
        return angle < 0 ? angle + 2 * .pi : angle
    }

    static func segmentIndex(angle: CGFloat, count: Int) -> Int {
        let segment = 2 * .pi / CGFloat(count)
        return Int((angle + segment / 2) / segment) % count
    }

    static func segmentBounds(index: Int, count: Int) -> (start: CGFloat, end: CGFloat) {
        let segment = 2 * .pi / CGFloat(count)
        let mid = CGFloat(index) * segment
        return (mid - segment / 2, mid + segment / 2)
    }

    typealias Bounds = (start: CGFloat, end: CGFloat)

    static func windowArcBounds(appIndex: Int, appCount: Int, windowCount: Int) -> [Bounds] {
        onScreen(fan(center: CGFloat(appIndex) * 2 * .pi / CGFloat(appCount), count: windowCount,
                     segment: windowArcSegment, maxSpan: tabArcMaxSpan),
                 outerRadius: windowArcRadii.upperBound)
    }

    nonisolated(unsafe) static var visibleArea: CGRect?
    private static let popOutAllowance: CGFloat = 30

    private static func onScreen(_ bounds: [Bounds], outerRadius: CGFloat) -> [Bounds] {
        guard let area = visibleArea, !bounds.isEmpty else { return bounds }
        let radius = outerRadius + popOutAllowance
        func fits(_ shift: CGFloat) -> Bool {
            bounds.allSatisfy { segment in
                [segment.start, (segment.start + segment.end) / 2, segment.end].allSatisfy { angle in
                    area.contains(CGPoint(x: radius * sin(angle + shift), y: radius * cos(angle + shift)))
                }
            }
        }
        if fits(0) { return bounds }
        for step in 1...63 {
            let shift = CGFloat(step) * 0.05
            for candidate in [shift, -shift] where fits(candidate) {
                return bounds.map { ($0.start + candidate, $0.end + candidate) }
            }
        }
        return bounds
    }

    static func tabArc(
        appIndex: Int, appCount: Int, windowIndex: Int?, windowBounds: [Bounds], tabCount: Int
    ) -> (radii: ClosedRange<CGFloat>, innerEdge: CGFloat, bounds: [Bounds])? {
        guard tabCount >= 2 else { return nil }
        if windowBounds.isEmpty {
            let center = CGFloat(appIndex) * 2 * .pi / CGFloat(appCount)
            let bounds = fan(center: center, count: tabCount, segment: singleWindowTabArcSegment, maxSpan: tabArcMaxSpan)
            return (singleWindowTabArcRadii, outerRadii.upperBound,
                    onScreen(bounds, outerRadius: singleWindowTabArcRadii.upperBound))
        }
        guard let windowIndex, windowBounds.indices.contains(windowIndex) else { return nil }
        let center = (windowBounds[windowIndex].start + windowBounds[windowIndex].end) / 2
        let bounds = fan(center: center, count: tabCount, segment: tabArcSegment, maxSpan: tabArcMaxSpan)
        return (tabArcRadii, windowArcRadii.upperBound, onScreen(bounds, outerRadius: tabArcRadii.upperBound))
    }

    private static func fan(center: CGFloat, count: Int, segment: CGFloat, maxSpan: CGFloat = fanMaxSpan) -> [Bounds] {
        let segment = min(segment, maxSpan / CGFloat(count))
        let start = center - segment * CGFloat(count) / 2
        return (0..<count).map { i in (start + CGFloat(i) * segment, start + CGFloat(i + 1) * segment) }
    }

    private static func fanIndex(angle: CGFloat, bounds: [Bounds]) -> Int? {
        guard let first = bounds.first, let last = bounds.last else { return nil }
        let segment = first.end - first.start
        let span = last.end - first.start
        let origin = first.start - segment / 2
        var relative = (angle - origin).truncatingRemainder(dividingBy: 2 * .pi)
        if relative < 0 { relative += 2 * .pi }
        guard relative <= span + segment else { return nil }
        return min(max(Int((relative - segment / 2) / segment), 0), bounds.count - 1)
    }

    static let closeButtonEdge: CGFloat = 6
    static let closeButtonHitRadius: CGFloat = 13

    static func closeButtonRadius(radii: ClosedRange<CGFloat>, pop: CGFloat, scale: CGFloat) -> CGFloat {
        let mid = (radii.lowerBound + radii.upperBound) / 2
        return mid + (radii.upperBound - closeButtonEdge - pop - mid) / scale
    }

    static func closeButtonOffset(bounds: Bounds, radii: ClosedRange<CGFloat>, isFan: Bool) -> CGSize {
        let mid = (bounds.start + bounds.end) / 2
        let scale: CGFloat = isFan ? 1.2 : 1
        let popDistance: CGFloat = isFan ? 10 : 7
        let button = offset(angle: mid, radius: closeButtonRadius(radii: radii, pop: popDistance, scale: scale))
        let center = offset(angle: mid, radius: (radii.lowerBound + radii.upperBound) / 2)
        let pop = offset(angle: mid, radius: popDistance)
        return CGSize(width: center.width + scale * (button.width - center.width) + pop.width,
                      height: center.height + scale * (button.height - center.height) + pop.height)
    }

    struct Aim {
        let arc: Bounds
        let arcInner: CGFloat
    }

    private static let aimMargin: CGFloat = 0.2
    private static let aimSamples = 24

    static func isHeading(toward aim: Aim, from: CGPoint, to: CGPoint) -> Bool {
        let move = CGPoint(x: to.x - from.x, y: to.y - from.y)
        guard hypot(to.x, to.y) < aim.arcInner, hypot(move.x, move.y) > 0 else { return false }
        let direction = atan2(move.y, move.x)
        func wrap(_ a: CGFloat) -> CGFloat {
            var r = (a + .pi).truncatingRemainder(dividingBy: 2 * .pi)
            if r < 0 { r += 2 * .pi }
            return r - .pi
        }
        let relative = (0...aimSamples).map { k -> CGFloat in
            let angle = aim.arc.start + (aim.arc.end - aim.arc.start) * CGFloat(k) / CGFloat(aimSamples)
            let point = CGPoint(x: aim.arcInner * sin(angle), y: aim.arcInner * cos(angle))
            return wrap(atan2(point.y - to.y, point.x - to.x) - direction)
        }
        if relative.contains(where: { abs($0) <= aimMargin }) { return true }
        return zip(relative, relative.dropFirst()).contains { a, b in (a < 0) != (b < 0) && abs(a - b) < .pi }
    }

    static func offset(angle: CGFloat, radius: CGFloat) -> CGSize {
        CGSize(width: radius * sin(angle), height: -radius * cos(angle))
    }
}
