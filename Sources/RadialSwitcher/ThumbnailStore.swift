import AppKit

@MainActor
final class ThumbnailStore {
    private let model: RingModel
    private let directory: URL
    private var lastWritten: [String: Date] = [:]

    private static let rewriteInterval: TimeInterval = 30
    private static let maxFileAge: TimeInterval = 3 * 24 * 3600

    init(model: RingModel) {
        self.model = model
        directory = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("RadialSwitcher/thumbnails", isDirectory: true)
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    }

    static func tabKey(windowID: CGWindowID, title: String) -> String {
        "\(windowID)-\(stableHash(title))"
    }

    func record(_ image: CGImage, windowID: CGWindowID, title: String) {
        let nsImage = NSImage(cgImage: image, size: .zero)
        model.thumbnails[windowID] = nsImage
        persist(image, name: "\(windowID)")

        guard model.tabsByWindow[windowID] != nil else { return }
        let tabKey = Self.tabKey(windowID: windowID, title: title)
        model.tabThumbnails[tabKey] = nsImage
        persist(image, name: tabKey)
    }

    func prune(liveWindowIDs: Set<CGWindowID>) {
        var liveTabKeys = Set<String>()
        for (id, tabs) in model.tabsByWindow where liveWindowIDs.contains(id) {
            for tab in tabs { liveTabKeys.insert(Self.tabKey(windowID: id, title: tab.title)) }
        }
        model.thumbnails = model.thumbnails.filter { liveWindowIDs.contains($0.key) }
        model.tabThumbnails = model.tabThumbnails.filter { liveTabKeys.contains($0.key) }

        let keep = liveTabKeys.union(liveWindowIDs.map { "\($0)" })
        lastWritten = lastWritten.filter { keep.contains($0.key) }
        let directory = directory
        Task.detached(priority: .utility) {
            let files = (try? FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil)) ?? []
            for file in files where !keep.contains(file.deletingPathExtension().lastPathComponent) {
                try? FileManager.default.removeItem(at: file)
            }
        }
    }

    func loadFromDisk() async {
        let directory = directory
        let maxAge = Self.maxFileAge
        let loaded: [(String, CGImage)] = await Task.detached(priority: .utility) {
            let files = (try? FileManager.default.contentsOfDirectory(
                at: directory, includingPropertiesForKeys: [.contentModificationDateKey])) ?? []
            var result: [(String, CGImage)] = []
            for file in files where file.pathExtension == "jpg" {
                let modified = (try? file.resourceValues(forKeys: [.contentModificationDateKey]))?.contentModificationDate
                if let modified, Date().timeIntervalSince(modified) > maxAge {
                    try? FileManager.default.removeItem(at: file)
                    continue
                }
                guard let source = CGImageSourceCreateWithURL(file as CFURL, nil),
                      let image = CGImageSourceCreateImageAtIndex(source, 0, nil)
                else { continue }
                result.append((file.deletingPathExtension().lastPathComponent, image))
            }
            return result
        }.value

        for (name, image) in loaded {
            let nsImage = NSImage(cgImage: image, size: .zero)
            if let id = CGWindowID(name) {
                if model.thumbnails[id] == nil { model.thumbnails[id] = nsImage }
            } else if model.tabThumbnails[name] == nil {
                model.tabThumbnails[name] = nsImage
            }
            lastWritten[name] = Date()
        }
        print("Loaded \(loaded.count) saved thumbnails")
    }

    private func persist(_ image: CGImage, name: String) {
        if let last = lastWritten[name], Date().timeIntervalSince(last) < Self.rewriteInterval { return }
        lastWritten[name] = Date()
        let url = directory.appendingPathComponent(name).appendingPathExtension("jpg")
        Task.detached(priority: .utility) {
            let data = NSBitmapImageRep(cgImage: image).representation(using: .jpeg, properties: [.compressionFactor: 0.75])
            try? data?.write(to: url, options: .atomic)
        }
    }

    private static func stableHash(_ string: String) -> String {
        var hash: UInt64 = 0xcbf2_9ce4_8422_2325
        for byte in string.utf8 {
            hash ^= UInt64(byte)
            hash = hash &* 0x100_0000_01b3
        }
        return String(hash, radix: 16)
    }
}
