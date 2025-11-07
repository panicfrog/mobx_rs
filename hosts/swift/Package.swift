// swift-tools-version: 6.2

import CompilerPluginSupport
import PackageDescription

let package = Package(
    name: "MobxSwift",
    platforms: [
        .iOS(.v14),
        .macOS(.v13),
    ],
    products: [
        .library(name: "MobxSwift", targets: ["MobxSwift"])
    ],
    dependencies: [
        .package(url: "https://github.com/apple/swift-syntax.git", from: "602.0.0"),
        .package(url: "https://github.com/apple/swift-atomics.git", from: "1.2.0"),
    ],
    targets: [
        .binaryTarget(name: "MobxRSFFI", path: "../../dist/MobxRS.xcframework.zip"),
        .macro(
            name: "MobxSwiftMacros",
            dependencies: [
                .product(name: "SwiftSyntaxMacros", package: "swift-syntax"),
                .product(name: "SwiftCompilerPlugin", package: "swift-syntax"),
                .product(name: "SwiftSyntaxBuilder", package: "swift-syntax"),
            ],
            path: "Sources/MobxSwiftMacros"
        ),
        .target(
            name: "MobxSwift",
            dependencies: [
                "MobxRSFFI",
                "MobxSwiftMacros",
                .product(name: "Atomics", package: "swift-atomics"),
            ],
            path: "Sources/MobxSwift"
        ),
        .testTarget(
            name: "MobxSwiftTests",
            dependencies: ["MobxSwift"],
            path: "Tests/MobxSwiftTests"
        ),
    ]
)
