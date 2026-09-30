---
name: flutter-studio-build
description: Use when building anywork on Windows or Linux, manually validating the native GUI with Flutter Driver (including headless SSH/Xvfb), debugging release builds, or troubleshooting flutter_rust_bridge generated bindings.
category: guides
platforms: [windows, linux]
---

# anywork 构建

## 标准入口

所有 GUI 构建和运行都从仓库根目录通过 xtask 执行：

```powershell
cargo xtask run-gui
cargo xtask run-gui --demo
cargo xtask run-gui --driver
cargo xtask run-gui --release
cargo xtask build-gui
cargo xtask build-gui --demo
cargo xtask build-gui --no-clean
cargo xtask build-gui --check-generated
```

当前产品只支持 Windows 与 Linux 桌面工程。不要直接执行 `flutter build windows|linux` 或
`flutter run -d windows|linux`；xtask 负责 Rust bridge 预构建、环境注入、Flutter 调用和进程回收。

普通 xtask 不链接 Studio runtime、模型、数据库或 GUI 验收工具。`cargo xtask manual-gui`
仅在调用时编译并启动独立 `pl-studio-acceptance`，参数由该工具唯一解析；路径与平台进程
支持由轻量 `pl-dev-support` 共享。验收工具使用正常 workspace 构建目录并尊重显式
`CARGO_TARGET_DIR`，不复用 xtask alias 专用的 `target/xtask` 参数。全 workspace 门禁
仍会编译验收工具；GUI 构建显式编译 bridge 也仍会编译 runtime，不应误判为依赖回流。

## 依赖与生成文件

`run-gui --release` 仅运行 `dist/anywork-release` 中已有的当前平台原生程序，不执行构建、生成或依赖解析。缺少程序时先执行 `build-gui`；不要将 demo 构建当作真实发布程序。关闭和取消沿用平台进程树所有权。

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
`ANYWORK_BRIDGE_LIBRARY` 等环境变量交给 CMake staging。CMake 不再启动 Cargo。

Linux remote helper 不嵌入 bridge：xtask 独立构建、校验并准备两种架构的压缩资源，CMake
随 Flutter bundle 安装到可执行文件旁的 `data/remote-helper/<target>/`。运行时从该目录
按架构加载资源并校验元数据与摘要，不依赖构建环境变量，不回退到 PATH 或源码目录。
排查缺资源时检查完整 bundle，而不是仅复制 bridge 动态库。仅 helper 资源变化不应导致
runtime 重编译或 bridge 重链接；用 Cargo Fresh 状态与 bridge 摘要验证，不单靠构建耗时。

绑定不同步常表现为 Dart 方法缺失、联合类型匹配不完整或 Rust/Dart 类型字段不一致。先同步生成
文件，再分别核验：

```powershell
cargo build -p pl-studio-bridge
cargo xtask verify-gui
```

## 构建产物

Flutter 原始 release 输出位于：

- Windows：`code/anywork/build/windows/x64/runner/Release/`
- Linux：`code/anywork/build/linux/x64/release/bundle/`

`cargo xtask build-gui` 完成后再把可发布文件收集到 `dist/anywork-release/`。讨论故障时明确区分
Flutter 原始输出与 xtask 最终收集目录。

Windows 发布目录通常包含 `anywork.exe`、`flutter_windows.dll`、
`pl_studio_bridge.dll`、可选 PDB 以及 `data/`。Linux 发布目录包含对应可执行文件、Flutter 库、
`libpl_studio_bridge.so` 与数据目录。两平台 `data/remote-helper/` 均包含两个架构的
`pl-remote-helper.zst` 和 `pl-remote-helper.metadata.json`，应随安装包一并交付。

## 按任务选择验证

- 只要求构建或运行时，完成对应 xtask 命令并核实产物或启动结果；不自动扩展为完整验收。
- 生成输入变更时先执行 `cargo xtask generate-gui`，再执行 `cargo xtask check-gui-generated`。
- GUI 或桥接修改执行 `cargo xtask verify-gui`；GUI 行为变更按根 `AGENTS.md` 执行
  `cargo xtask manual-gui` 与相应 Flutter Driver 验收。`verify-gui` 不接受 `--integration`。
- 授权、提交前门禁与完成条件以根 `AGENTS.md` 为准；适用检查通过后直接交付，只有新修改、
  失败或具体未决风险才重跑。已通过的生成检查不另行重复，门禁命令内部自带的检查正常保留。

Linux 缺少编译器、CMake、Ninja、pkg-config 或 GTK 3 开发文件时，保留 xtask 返回的真实预检命令
和原始错误，不注入机器专用 include/library 路径。

## 原生 GUI 人工验收

执行前读取 [人工验收操作指南](references/manual-gui.md)：包含隔离 harness、Linux SSH 下的
伪终端与 Xvfb、Flutter Driver 定向操作、截图/快照核对及进程回收。不要将无交互终端直接
等同于无法验收，也不要把 demo、静态分析或成功启动等同于完整业务验收。
