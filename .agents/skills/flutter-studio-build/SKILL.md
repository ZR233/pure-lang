---
name: flutter-studio-build
description: Use when building Pure Studio on Windows or Linux, debugging release builds, or troubleshooting flutter_rust_bridge generated bindings.
category: guides
platforms: [windows, linux]
---

# Pure Studio 构建

## 标准入口

所有 GUI 构建和运行都从仓库根目录通过 xtask 执行：

```powershell
cargo xtask run-gui
cargo xtask run-gui --demo
cargo xtask run-gui --driver
cargo xtask build-gui
cargo xtask build-gui --demo
cargo xtask build-gui --no-clean
cargo xtask build-gui --check-generated
```

当前产品只支持 Windows 与 Linux 桌面工程。不要直接执行 `flutter build windows|linux` 或
`flutter run -d windows|linux`；xtask 负责 Rust bridge 预构建、环境注入、Flutter 调用和进程回收。

## 依赖与生成文件

`run-gui` 和 `build-gui` 在 `.dart_tool/pure-xtask-pub.sha256` 记录 `pubspec.yaml`、
`pubspec.lock`、`pubspec_overrides.yaml` 与 `PUB_HOSTED_URL` 的依赖指纹；指纹未变时使用
Flutter `--no-pub` 热路径。

普通运行和构建不执行生成器或可写格式化。Riverpod、Freezed、本地化或 FRB 输入变化后先运行：

```powershell
cargo xtask generate-gui
cargo xtask check-gui-generated
```

`check-gui-generated`、`verify-gui` 与 `build-gui --check-generated` 比较重新生成前后的 canonical
输出，拒绝生成不稳定或输入未同步；它们不以 Git 暂存或提交状态作为判断依据。生成文件不得手工
修改，也不得绕过 xtask 直接调用单个生成器。

## Rust bridge

`pl-studio-bridge` 由 Cargo 预构建；flutter_rust_bridge 负责生成 Dart/Rust 绑定，不负责生成
DLL 或共享库。xtask 从 Cargo profile 目录定位 Windows DLL/PDB 或 Linux `.so`，通过
`PURE_STUDIO_BRIDGE_LIBRARY` 等环境变量交给 CMake staging。CMake 不再启动 Cargo。

绑定不同步常表现为 Dart 方法缺失、联合类型匹配不完整或 Rust/Dart 类型字段不一致。先同步生成
文件，再分别核验：

```powershell
cargo build -p pl-studio-bridge
cargo xtask verify-gui
```

## 构建产物

Flutter 原始 release 输出位于：

- Windows：`code/pure-studio/build/windows/x64/runner/Release/`
- Linux：`code/pure-studio/build/linux/x64/release/bundle/`

`cargo xtask build-gui` 完成后再把可发布文件收集到 `dist/pure-studio-release/`。讨论故障时明确区分
Flutter 原始输出与 xtask 最终收集目录。

Windows 发布目录通常包含 `pure_studio.exe`、`flutter_windows.dll`、
`pl_studio_bridge.dll`、可选 PDB 以及 `data/`。Linux 发布目录包含对应可执行文件、Flutter 库、
`libpl_studio_bridge.so` 与数据目录。

## 验证顺序

1. 生成输入变更时先执行 `cargo xtask generate-gui`。
2. 执行 `cargo xtask check-gui-generated`，确认生成稳定。
3. 执行 `cargo xtask verify-gui` 完成格式、分析、测试和桥接检查。
4. 原生行为变更时执行 `cargo xtask verify-gui --integration`。
5. 需要确定性交互验收时运行 `cargo xtask run-gui --demo --driver`。

Linux 缺少编译器、CMake、Ninja、pkg-config 或 GTK 3 开发文件时，保留 xtask 返回的真实预检命令
和原始错误，不注入机器专用 include/library 路径。
