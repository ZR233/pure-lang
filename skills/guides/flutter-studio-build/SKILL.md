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
```

`pl-xtask` 自动检测当前 OS，并在 `code/pure-studio/` 目录下执行对应 `flutter build <platform> --release`，产物收集到 `dist/pure-studio-release/`。

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
| Windows | `flutter build windows --release` | `code/pure-studio/build/windows/x64/runner/Release/` |
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
cd code/pure-studio
flutter_rust_bridge_codegen generate
```

> 当前项目使用 `flutter_rust_bridge = "=2.12.0"`（在 `Cargo.toml` 中锁定）。`flutter_rust_bridge_codegen` 版本必须与此匹配，否则生成代码可能与运行时库不兼容。

**触发场景**：
- 新增/修改/删除 `rust/src/api/` 中的 public 函数
- 新增/修改/删除通过 bridge 暴露的 enum 变体
- 新增/修改/删除通过 bridge 暴露的 struct 字段
- flutter_rust_bridge 版本升级

### 2. Rust 编译失败（release 模式）

`pl-studio-bridge` 作为 `cdylib` 输出，通过 CMake 集成到 Flutter Windows 构建中。如果 `cargo build -p pl-studio-bridge --release` 成功但在 Flutter 构建中失败，差异点通常是：

- 不同工作目录下的 `.cargo/config.toml` 生效范围
- Flutter 构建系统使用独立的 Rust target 目录：`code/pure-studio/build/windows/x64/rust-target/`
- 上游 workspace crate（`pl-core`、`pl-model`、`pl-protocol`）是否已提前编译

先用独立 `cargo build -p pl-studio-bridge --release` 验证 Rust 侧能否单独通过，再确认 Flutter 构建。

### 3. Demo 模式构建

`flutter build windows --release --dart-define=PURE_STUDIO_DEMO=true` 会在编译时注入 `PURE_STUDIO_DEMO` 常量，使 Flutter app 在无 Rust 后端时以纯 UI 演示模式运行。用于：
- 前端开发调试
- 后端 bridge 代码尚未完成时预览 UI
- CI 中分离前后端验证

需要 Demo 数据时显式运行 `cargo xtask run-gui --demo`。Native 运行失败会直接报错，不会切换到另一套运行路径。

## CI 参考

`.github/workflows/rc-build.yml` 展示了 CI 中的 release 构建流程：

```yaml
- name: flutter pub get
  working-directory: code/pure-studio
  run: flutter pub get
- name: flutter build windows
  working-directory: code/pure-studio
  run: flutter build windows --release
- name: pack artifact (windows)
  run: |
    Copy-Item code\pure-studio\build\windows\x64\runner\Release\* dist -Recurse -Force
```

## 开发构建 vs Release 构建

| 方面 | `cargo xtask run-gui` | `cargo xtask build-gui` |
|------|-------------------------------|----------------------------------------|
| 用途 | 开发运行/调试 | 产出 release 包 |
| 构建模式 | `flutter run -d windows`（debug） | `flutter build windows --release` |
| 产物 | 不收集，在 `build/` 下就地运行 | 收集到 `dist/pure-studio-release/` |
| Demo 模式 | 使用 `--demo` 显式选择 | Native 失败即报错 |
