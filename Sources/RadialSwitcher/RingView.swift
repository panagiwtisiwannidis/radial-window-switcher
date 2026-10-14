import SwiftUI

struct RingOverlayView: View {
    @ObservedObject var model: RingModel

    var body: some View {
        ZStack {
            Color.black.opacity(0.02)
            RingView(model: model)
                .position(model.center)
        }
        .ignoresSafeArea()
    }
}

enum RingAnimation {
    static let appear = Animation.spring(response: 0.28, dampingFraction: 0.72)
    static let hover = Animation.smooth(duration: 0.28)
    static let arc = Animation.spring(response: 0.24, dampingFraction: 0.8)
}

struct RingView: View {
    @ObservedObject var model: RingModel

    private let diameter = RingGeometry.maxRadius * 2

    var body: some View {
        ZStack {
            if let tabsWindow = model.innerTabsWindowIndex {
                ring(model.outer, level: .outer, radii: RingGeometry.outerRadii, interactive: false)
                    .backdrop(true)
                ring(model.inner, level: .inner, radii: RingGeometry.innerRadii)
                ZStack { tabPopup(innerIndex: tabsWindow, tabs: model.outerItems) }
                    .shadow(color: .black.opacity(0.3), radius: 14)
                    .id("popup-\(tabsWindow)")
                    .transition(.scale(scale: 0.88).combined(with: .opacity))
            } else {
                ring(model.outer, level: .outer, radii: RingGeometry.outerRadii)
                    .backdrop(isPastAppsRing)
                ring(model.inner, level: .inner, radii: RingGeometry.innerRadii)
                    .backdrop(isPastAppsRing)
                if let appIndex = model.expandedAppIndex {
                    ZStack { windowArc(appIndex: appIndex) }
                        .id("windows-\(appIndex)")
                        .transition(.scale(scale: 0.92).combined(with: .opacity))
                }
                if let arc = model.openTabArc {
                    ZStack { tabArc(appIndex: arc.appIndex, windowIndex: arc.windowIndex, tabs: arc.items) }
                        .shadow(color: .black.opacity(isPastAppsRing ? 0.3 : 0), radius: 14)
                        .id("tabs-\(arc.appIndex)-\(arc.windowIndex ?? -1)")
                        .transition(.scale(scale: 0.92).combined(with: .opacity))
                }
            }
            hub
            if let item = model.selectedItem {
                PreviewCard(item: item, thumbnail: previewThumbnail(for: item))
                    .offset(x: model.previewOffsetX)
                    .transition(.opacity)
            }
        }
        .frame(width: diameter, height: diameter)
        .shadow(color: .black.opacity(0.18), radius: 12, y: 4)
        .animation(RingAnimation.arc, value: model.innerTabsWindowIndex)
        .animation(RingAnimation.arc, value: model.expandedAppIndex)
        .animation(RingAnimation.arc, value: openTabArcKey)
        .animation(RingAnimation.arc, value: isPastAppsRing)
        .scaleEffect(model.isShown ? 1 : 0.82)
        .opacity(model.isShown ? 1 : 0)
    }

    private var isPastAppsRing: Bool {
        guard model.innerTabsWindowIndex == nil, let selection = model.selection, selection.level == .outer else { return false }
        return selection.windowIndex != nil || selection.tabIndex != nil
    }

    private var openTabArcKey: String? {
        model.openTabArc.map { "\($0.appIndex)-\($0.windowIndex ?? -1)" }
    }

    private func ring(_ items: [RingItem], level: RingLevel, radii: ClosedRange<CGFloat>, interactive: Bool = true) -> some View {
        ForEach(Array(items.enumerated()), id: \.element.id) { index, item in
            let isSelected = interactive && model.selection == RingSelection(level: level, index: index)
            let isParentOfSelection = interactive && model.selection?.level == level && model.selection?.index == index
                && (model.selection?.windowIndex != nil || model.selection?.tabIndex != nil
                    || model.selection?.isPointing == false)
                || level == .inner && model.innerTabsWindowIndex == index && model.selection?.level == .outer
            let bounds = RingGeometry.segmentBounds(index: index, count: items.count)
            Segment(bounds: bounds, radii: radii, gap: items.count > 1 ? 3 : 0,
                    state: isSelected ? .selected : isParentOfSelection ? .parent : .normal,
                    isCloseHovered: model.isCloseHovered) {
                if case .tab = item.target {
                    TabArcLabel(item: item, thumbnail: previewThumbnail(for: item), isSelected: isSelected,
                                width: (bounds.end - bounds.start) * (radii.lowerBound + radii.upperBound) / 2 - 8)
                } else {
                    SegmentLabel(item: item, level: level, isSelected: isSelected,
                                 iconSize: iconSize(count: items.count, radii: radii))
                }
            }
        }
    }

    private func windowArc(appIndex: Int) -> some View {
        let app = model.outer[appIndex]
        let bounds = RingGeometry.windowArcBounds(appIndex: appIndex, appCount: model.outer.count, windowCount: app.windows.count)
        return ForEach(Array(app.windows.enumerated()), id: \.element.id) { index, window in
            let isSelected = model.selection?.windowIndex == index && model.selection?.tabIndex == nil
                && model.selection?.isPointing == true
            let isParentOfSelection = model.selection?.windowIndex == index
                && (model.selection?.tabIndex != nil || model.selection?.isPointing == false)
            let radii = RingGeometry.windowArcRadii
            Segment(bounds: bounds[index], radii: radii, gap: 3,
                    state: isSelected ? .selected : isParentOfSelection ? .parent : .normal, emphasis: .tab,
                    isCloseHovered: model.isCloseHovered) {
                TabArcLabel(item: window, thumbnail: previewThumbnail(for: window), isSelected: isSelected,
                            width: (bounds[index].end - bounds[index].start) * (radii.lowerBound + radii.upperBound) / 2 - 8,
                            thumbnailHeight: 66)
            }
        }
    }

    private func tabArc(appIndex: Int, windowIndex: Int?, tabs: [RingItem]) -> some View {
        let windowCount = model.outer[appIndex].windows.count
        let windowBounds = windowCount >= 2
            ? RingGeometry.windowArcBounds(appIndex: appIndex, appCount: model.outer.count, windowCount: windowCount)
            : []
        let arc = RingGeometry.tabArc(appIndex: appIndex, appCount: model.outer.count, windowIndex: windowIndex,
                                      windowBounds: windowBounds, tabCount: tabs.count)
        return ForEach(Array(tabs.enumerated()), id: \.element.id) { index, tab in
            if let arc {
                let isSelected = model.selection?.tabIndex == index
                let bounds = arc.bounds[index]
                let midRadius = (arc.radii.lowerBound + arc.radii.upperBound) / 2
                Segment(bounds: bounds, radii: arc.radii, gap: 2, state: isSelected ? .selected : .normal, emphasis: .tab,
                        isCloseHovered: model.isCloseHovered) {
                    TabArcLabel(item: tab, thumbnail: previewThumbnail(for: tab), isSelected: isSelected,
                                width: (bounds.end - bounds.start) * midRadius - 8, thumbnailHeight: 66)
                }
            }
        }
    }

    private func tabPopup(innerIndex: Int, tabs: [RingItem]) -> some View {
        let radii = RingGeometry.tabPopupRadii
        let midRadius = (radii.lowerBound + radii.upperBound) / 2
        let bounds = RingGeometry.tabPopupBounds(innerIndex: innerIndex, innerCount: model.inner.count, tabCount: tabs.count)
        return ForEach(Array(tabs.enumerated()), id: \.element.id) { index, tab in
            let isSelected = model.selection == RingSelection(level: .outer, index: index)
            Segment(bounds: bounds[index], radii: radii, gap: 3, state: isSelected ? .selected : .normal, emphasis: .tab,
                    isCloseHovered: model.isCloseHovered) {
                TabArcLabel(item: tab, thumbnail: previewThumbnail(for: tab), isSelected: isSelected,
                            width: (bounds[index].end - bounds[index].start) * midRadius - 8, thumbnailHeight: 66)
            }
        }
    }

    private func previewThumbnail(for item: RingItem) -> NSImage? {
        guard let id = item.previewWindow?.windowID else { return nil }
        if case .tab(let tab, _) = item.target {
            if tab.isActive, let live = model.liveFrame, live.windowID == id { return live.image }
            let tabSnapshot = model.tabThumbnails[ThumbnailStore.tabKey(windowID: id, title: tab.title)]
            return tabSnapshot ?? (tab.isActive ? model.thumbnails[id] : nil)
        }
        if let live = model.liveFrame, live.windowID == id { return live.image }
        return model.thumbnails[id]
    }

    private func iconSize(count: Int, radii: ClosedRange<CGFloat>) -> CGFloat {
        let arcLength = 2 * .pi * (radii.lowerBound + radii.upperBound) / 2 / CGFloat(max(count, 1))
        return min(42, max(18, arcLength - 16))
    }

    private var hub: some View {
        ZStack {
            GlassBackground(shape: Circle(), state: .normal)
                .frame(width: RingGeometry.deadZoneRadius * 2 - 6, height: RingGeometry.deadZoneRadius * 2 - 6)
            if let item = model.selectedItem {
                Image(nsImage: item.icon)
                    .resizable()
                    .frame(width: 44, height: 44)
                    .id(item.id)
                    .transition(.scale(scale: 0.6).combined(with: .opacity))
            }
        }
        .animation(RingAnimation.hover, value: model.selectedItem?.id)
    }
}

enum SegmentState { case normal, parent, selected }

private struct Segment<Label: View>: View {
    enum Emphasis {
        case normal, tab

        var popDistance: CGFloat { self == .tab ? 10 : 7 }
        var labelScale: CGFloat { self == .tab ? 1.12 : 1.15 }
        var segmentScale: CGFloat { self == .tab ? 1.2 : 1 }
    }

    let bounds: RingGeometry.Bounds
    let radii: ClosedRange<CGFloat>
    let gap: CGFloat
    let state: SegmentState
    var emphasis: Emphasis = .normal
    var isCloseHovered = false
    @ViewBuilder let label: () -> Label

    var body: some View {
        let midAngle = (bounds.start + bounds.end) / 2
        let midRadius = (radii.lowerBound + radii.upperBound) / 2
        let sector = AnnularSector(startAngle: bounds.start, endAngle: bounds.end,
                                   innerRadius: radii.lowerBound, outerRadius: radii.upperBound, gap: gap)
        let isSelected = state == .selected
        let labelOffset = RingGeometry.offset(angle: midAngle, radius: midRadius)
        let diameter = RingGeometry.maxRadius * 2
        let segmentCenter = UnitPoint(x: 0.5 + labelOffset.width / diameter, y: 0.5 + labelOffset.height / diameter)
        ZStack {
            GlassBackground(shape: sector, state: state)
            label()
                .scaleEffect(isSelected ? emphasis.labelScale : 1)
                .offset(labelOffset)
            if isSelected {
                CloseBadge(isHovered: isCloseHovered)
                    .offset(RingGeometry.offset(angle: midAngle, radius: RingGeometry.closeButtonRadius(
                        radii: radii, pop: emphasis.popDistance, scale: emphasis.segmentScale)))
                    .transition(.scale(scale: 0.5).combined(with: .opacity))
            }
        }
        .scaleEffect(isSelected ? emphasis.segmentScale : 1, anchor: segmentCenter)
        .offset(isSelected ? RingGeometry.offset(angle: midAngle, radius: emphasis.popDistance) : .zero)
        .zIndex(isSelected ? 1 : 0)
        .animation(RingAnimation.hover, value: state)
    }
}

private struct CloseBadge: View {
    let isHovered: Bool

    var body: some View {
        ZStack {
            Circle()
                .fill(Color.black.opacity(0.55))
            Circle()
                .stroke(Color.white.opacity(0.7), lineWidth: 1)
            Image(systemName: "xmark")
                .font(.system(size: 8, weight: .bold))
                .foregroundStyle(.white)
        }
        .frame(width: 18, height: 18)
        .scaleEffect(isHovered ? 1.3 : 1)
        .animation(RingAnimation.hover, value: isHovered)
    }
}

private struct GlassBackground<S: Shape>: View {
    let shape: S
    let state: SegmentState

    var body: some View {
        if #available(macOS 26.0, *) {
            Color.clear
                .glassEffect(.clear, in: shape)
                .overlay(shape.fill(Color.accentColor).opacity(tintOpacity))
                .overlay(shape.stroke(Self.rimLight, lineWidth: 1))
        } else {
            shape.fill(.ultraThinMaterial)
                .overlay(shape.fill(Color.accentColor).opacity(tintOpacity))
                .overlay(shape.stroke(Color.primary.opacity(0.12), lineWidth: 1))
        }
    }

    private static var rimLight: LinearGradient {
        LinearGradient(colors: [.white.opacity(0.55), .white.opacity(0.12), .white.opacity(0.04)],
                       startPoint: .topLeading, endPoint: .bottomTrailing)
    }

    private var tintOpacity: Double {
        switch state {
        case .selected: 0.8
        case .parent: 0.28
        case .normal: 0
        }
    }
}

enum PreviewLayout {
    static let imageSize = CGSize(width: 440, height: 290)
    static let padding: CGFloat = 12
    static let cardWidth = imageSize.width + padding * 2
    static let distanceFromCenter = RingGeometry.maxRadius + 16 + cardWidth / 2
}

private struct PreviewCard: View {
    let item: RingItem
    let thumbnail: NSImage?

    var body: some View {
        VStack(spacing: 8) {
            Group {
                if let thumbnail {
                    Image(nsImage: thumbnail)
                        .resizable()
                        .aspectRatio(contentMode: .fit)
                        .clipShape(RoundedRectangle(cornerRadius: 8))
                } else {
                    Image(nsImage: item.icon)
                        .resizable()
                        .frame(width: 96, height: 96)
                }
            }
            .frame(width: PreviewLayout.imageSize.width, height: PreviewLayout.imageSize.height)

            Text(item.fullTitle)
                .font(.system(size: 13, weight: .medium))
                .lineLimit(1)
                .truncationMode(.middle)
                .frame(maxWidth: PreviewLayout.imageSize.width)
                .readableOnGlass()
        }
        .padding(PreviewLayout.padding)
        .background(GlassBackground(shape: RoundedRectangle(cornerRadius: 18), state: .normal))
    }
}

private struct SegmentLabel: View {
    let item: RingItem
    let level: RingLevel
    let isSelected: Bool
    let iconSize: CGFloat

    var body: some View {
        Group {
            switch level {
            case .outer:
                Image(nsImage: item.icon)
                    .resizable()
                    .frame(width: iconSize, height: iconSize)
            case .inner:
                VStack(spacing: 2) {
                    Image(nsImage: item.icon)
                        .resizable()
                        .frame(width: 20, height: 20)
                    Text(item.shortTitle)
                        .font(.system(size: 9, weight: .medium))
                        .lineLimit(2)
                        .multilineTextAlignment(.center)
                        .frame(width: 62)
                        .foregroundStyle(isSelected ? Color.white : Color.primary)
                        .readableOnGlass()
                }
            }
        }
        .opacity(item.isDimmed && !isSelected ? 0.5 : 1)
    }
}

private struct TabArcLabel: View {
    let item: RingItem
    let thumbnail: NSImage?
    let isSelected: Bool
    let width: CGFloat
    var thumbnailHeight: CGFloat = 34

    var body: some View {
        let isActive: Bool = if case .tab(let tab, _) = item.target { tab.isActive } else { false }
        let width = max(width, 20)
        VStack(spacing: 2) {
            if let thumbnail {
                Image(nsImage: thumbnail)
                    .resizable()
                    .aspectRatio(contentMode: .fit)
                    .frame(maxWidth: width, maxHeight: thumbnailHeight)
                    .clipShape(RoundedRectangle(cornerRadius: 3))
            } else {
                Image(nsImage: item.icon)
                    .resizable()
                    .frame(width: 18, height: 18)
            }
            Text(item.shortTitle)
                .font(.system(size: 8, weight: isActive ? .bold : .regular))
                .lineLimit(thumbnail == nil ? 2 : 1)
                .multilineTextAlignment(.center)
                .frame(width: width)
                .foregroundStyle(isSelected ? Color.white : Color.primary)
                .readableOnGlass()
        }
    }
}

struct AnnularSector: Shape {
    let startAngle: CGFloat
    let endAngle: CGFloat
    let innerRadius: CGFloat
    let outerRadius: CGFloat
    let gap: CGFloat

    func path(in rect: CGRect) -> Path {
        let center = CGPoint(x: rect.midX, y: rect.midY)
        func swiftUIAngle(_ a: CGFloat) -> Angle { .radians(Double(a - .pi / 2)) }
        let outerInset = gap / 2 / outerRadius
        let innerInset = gap / 2 / innerRadius

        var path = Path()
        path.addArc(center: center, radius: outerRadius,
                    startAngle: swiftUIAngle(startAngle + outerInset), endAngle: swiftUIAngle(endAngle - outerInset),
                    clockwise: false)
        path.addArc(center: center, radius: innerRadius,
                    startAngle: swiftUIAngle(endAngle - innerInset), endAngle: swiftUIAngle(startAngle + innerInset),
                    clockwise: true)
        path.closeSubpath()
        return path
    }
}

private extension View {
    func backdrop(_ active: Bool) -> some View {
        blur(radius: active ? 3 : 0).opacity(active ? 0.35 : 1)
    }

    func readableOnGlass() -> some View {
        shadow(color: .black.opacity(0.45), radius: 1.5, y: 0.5)
    }
}
