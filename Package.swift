// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "RadialSwitcher",
    platforms: [.macOS(.v14)],
    targets: [
        .executableTarget(
            name: "RadialSwitcher",
            path: "Sources/RadialSwitcher"
        )
    ]
)
