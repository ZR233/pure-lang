---
name: flutter-studio-build
description: Use when building Pure Studio, debugging release builds, or troubleshooting flutter_rust_bridge code sync issues.
category: guides
platforms: [windows, linux, macos]
---

# Pure Studio Release Build

## 快速构建

项目根目录的 `cargo xtask build-gui` 一键执行 release 构建：

```powershell
cargo xtask build-gui            # 正常 release 构建
cargo xtask build-gui --demo     # Demo 模式（无需 Rust 后端）
cargo xtask build-gui --no-clean # 保留已存在的 release 输出目录
cargo xtask build-gui --check-generated # CI/发布拒绝未提交的生成差异
```

`pl-xtask` 自动检测当前 OS，并在 `code/pure-studio/` 目录下执行对应 `flutter build <platform> --release`，产物收集到 `dist/pure-studio-release/`。
`run-gui` 和 `build-gui` 会在 `.dart_tool/pure-xtask-pub.sha256` 记录 `pubspec.yaml`、
`pubspec.lock`、`pubspec_overrides.yaml` 和 `PUB_HOSTED_URL` 的依赖指纹；指纹未变时使用
Flutter `--no-pub` 热路径，不重复解析依赖或改写 lockfile。
普通 `run-gui` 和 `build-gui` 不执行生成器或可写格式化；Riverpod、Freezed、l10n 和 FRB
输入变化后必须先显式运行 `cargo xtask generate-gui`。`check-gui-generated`、`verify-gui`
以及 `build-gui --check-generated` 会重新生成并检查未提交输出。
Windows 下 xtask 先用普通前台 Cargo 命令在 workspace target 中构建 `pl-studio-bridge`，
编译过程会实时显示在当前终端；完成后从对应 profile 目录定位 DLL/PDB，再交给 CMake 复制。
CMake 不再启动 Cargo；直接执行 `flutter build/run windows` 会因缺少预编译 artifact 明确失败。

### 产物结构（Windows）

```
dist/pure-studio-release/
  pure_studio.exe     # Flutter 主程序
  flutter_windows.dll         # Flutter 引擎动态库
  pl_studio_bridge.dll        # Rust bridge DLL（通过 flutter_rust_bridge 生成）
  pl_studio_bridge.pdb        # Rust bridge 调试符号
  data/
    app.so                    # AOT 编译的 Dart 代码
    flutter_assets/           # Flutter 静态资源
    icudtl.dat                # ICU 国际化数据
```

### 各平台构建输出路径

| 平台 | 构建命令 | 产物目录 |
|------|---------|---------|
| Windows | `cargo xtask build-gui` | `code/pure-studio/build/windows/x64/runner/Release/` |
| macOS | `flutter build macos --release` | `code/pure-studio/build/macos/Build/Products/Release/` |
| Linux | `flutter build linux --release` | `code/pure-studio/build/linux/x64/release/bundle/` |

### Flutter 命令解析

`pl-xtask` 不查找 SDK 安装目录，直接调用 PATH 中的 `flutter`。Windows 下通过 `cmd /c flutter ...` 运行，以匹配终端对 `flutter.bat` 的解析行为。

## 常见故障模式

### 1. Bridge 代码不同步（最常见）

**错误信息示例**：
```
lib/src/data/frb/studio_api.dart: error: Method not found: 'saveStudioSettingsDraft'
lib/src/rust/frb_generated.dart: error: The type 'BridgeEventPayload' is not
  exhaustively matched by the switch cases since it doesn't match
  'BridgeEventPayload_SessionHandoffChanged()'
```

**根因**：Rust 侧 `pl-studio-bridge` 的 API 或类型（在 `code/pure-studio/rust/src/api/` 中）发生变更后，flutter_rust_bridge 生成的 Dart 代码未同步更新。

**修复步骤**：
```powershell
cargo xtask generate-gui
cargo xtask check-gui-generated
```

普通 GUI build/run 不会自动修复不同步绑定，以保证构建不会修改已跟踪源码。

> 生成文件不得手工修改，也不得直接调用单个生成器。当前项目使用
> `flutter_rust_bridge = "=2.12.0"`（在 `Cargo.toml` 中锁定），xtask 会校验
> `flutter_rust_bridge_codegen` 版本，避免生成代码与运行时库不兼容。

**触发场景**：
- 新增/修改/删除 `rust/src/api/` 中的 public 函数
- 新增/修改/删除通过 bridge 暴露的 enum 变体
- 新增/修改/删除通过 bridge 暴露的 struct 字段
- flutter_rust_bridge 版本升级

### 2. Rust 编译失败（release 模式）

`pl-studio-bridge` 作为 `cdylib` 输出，由 xtask 预编译后通过 CMake 集成到 Flutter Windows
构建中。如果 `cargo build -p pl-studio-bridge --release` 成功但 GUI 构建失败，重点检查：

- 不同工作目录下的 `.cargo/config.toml` 生效范围
- `PURE_STUDIO_BRIDGE_LIBRARY` 是否是 xtask 从 Cargo target/profile 定位的绝对 DLL 路径
- `PURE_STUDIO_BRIDGE_DEBUG_SYMBOLS` 指向的可选 PDB 是否仍存在
- 上游 workspace crate（`pl-core`、`pl-model`、`pl-protocol`）是否能在同一 target 中编译

`pl-xtask` 自身位于独立的 `target/xtask`，避免运行中的外层 Cargo 锁住 workspace target；
bridge 仍使用 workspace 默认 target，并尊重调用方的 `CARGO_TARGET_DIR`。重复 GUI 构建由
Cargo 自身判断 artifact 是否 fresh，CMake 只执行 `copy_if_different`。

先用独立 `cargo build -p pl-studio-bridge --release` 验证 Rust 侧能否单独通过，再确认 Flutter 构建。

### 3. Demo 模式构建

`cargo xtask build-gui --demo` 会注入 `PURE_STUDIO_DEMO` 常量，使 Flutter app 在无 Rust 后端时
以纯 UI 演示模式运行。用于：
- 前端开发调试
- 后端 bridge 代码尚未完成时预览 UI
- CI 中分离前后端验证

需要 Demo 数据时显式运行 `cargo xtask run-gui --demo`。Dart MCP GUI 验收使用
`cargo xtask run-gui --driver`；需要确定性数据时追加 `--demo`。driver 使用 resident 生命周期，
退出 xtask 后 Flutter、DTD 与 GUI 子进程必须一并结束。Native 运行失败会直接报错，不会切换到另一套运行路径。

## CI 参考

`.github/workflows/rc-build.yml` 展示了 CI 中的 release 构建流程：

```yaml
- name: build Pure Studio
  run: cargo xtask build-gui --check-generated
```

## 开发构建 vs Release 构建

| 方面 | `cargo xtask run-gui` | `cargo xtask build-gui` |
|------|-------------------------------|----------------------------------------|
| 用途 | 开发运行/调试 | 产出 release 包 |
| xtask 内部 Flutter 模式 | Windows debug run | Windows release build |
| 产物 | 不收集，在 `build/` 下就地运行 | 收集到 `dist/pure-studio-release/` |
| Demo 模式 | 使用 `--demo` 显式选择 | Native 失败即报错 |
