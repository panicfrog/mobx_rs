# MobxRS Swift Integration

## 1. 准备 Rust 产物

```bash
# 在仓库根目录
./scripts/build_rust_artifacts.sh
```

脚本会生成:
- `hosts/swift/Artifacts/macos/libmobx_rs.a`
- `hosts/swift/Artifacts/ios/device/libmobx_rs.a`
- `hosts/swift/Artifacts/ios/simulator/libmobx_rs.a`
- `hosts/swift/Artifacts/include/mobx_rs_ffi.h`

若只在 macOS 构建 Swift 包，仅需保证 `Artifacts/macos` 下的库存在即可。iOS 项目可手动拷贝相应 `libmobx_rs.a` 与头文件进入 Xcode 工程。

## 2. 生成 XCFramework

```bash
./scripts/build_swift_xcframework.sh
```

该脚本会运行 `build_rust_artifacts.sh` 并调用 `xcodebuild -create-xcframework`，在 `dist/` 目录输出 `MobxRS.xcframework`（以及同名 zip）。`Package.swift` 现已改为引用此 XCFramework，因此只要执行脚本即可让 SwiftPM 使用最新二进制。

若更倾向于直接链接静态库，可忽略 XCFramework，将 `Artifacts/include` 与对应平台的 `libmobx_rs.a` 手动加入 Xcode 工程。

## 3. 常用命令

```bash
# 在 hosts/swift 目录
swift build
swift test
```

运行上述命令前请确保已生成 `dist/MobxRS.xcframework`。在 Xcode 中将 `MobxRS` 作为本地 Swift Package 引入即可完成集成。
