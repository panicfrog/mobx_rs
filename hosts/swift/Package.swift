// swift-tools-version: 6.2

import PackageDescription

let package = Package(
    name: "MobxSwift",
    platforms: [
        .iOS(.v14),
        .macOS(.v13)
    ],
    products: [
        .library(name: "MobxSwift", targets: ["MobxSwift"])
    ],
    targets: [
        .binaryTarget(name: "MobxRSFFI", path: "../../dist/MobxRS.xcframework.zip"),
        .target(
            name: "MobxSwift",
            dependencies: ["MobxRSFFI"],
            path: "Sources/MobxSwift"
        ),
        .testTarget(
            name: "MobxSwiftTests",
            dependencies: ["MobxSwift"],
            path: "Tests/MobxSwiftTests"
        )
    ]
)
