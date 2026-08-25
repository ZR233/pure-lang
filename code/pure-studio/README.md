# Pure Studio Flutter

Windows-first Flutter desktop client for Pure Studio.

## Stack

- Flutter Material 3
- Riverpod state controller and selectors
- go_router page stack
- flutter_rust_bridge v2.12.x
- `pl-studio-bridge` Rust crate in `rust/`

## Commands

```powershell
# Run from repository root. xtask invokes Flutter with
# code/pure-studio as the working directory.
cargo flutter pub get
cargo flutter analyze
cargo flutter test
cargo dart format lib
cargo xtask run-gui
cargo xtask build-gui
cargo xtask generate-gui
cargo xtask check-gui-generated
cargo xtask build-gui --check-generated # CI/release: regenerate and reject uncommitted output

# Run the native app through the dedicated test_driver entrypoint for Dart MCP
# interaction and GUI acceptance. The driver command owns the resident process
# tree, keeps Flutter's control pipe open, and connects directly to the app VM
# service without a DDS proxy. Release builds never use it.
cargo xtask run-gui --driver
cargo xtask run-gui --demo --driver # deterministic demo data

# Windows GUI run/build must use xtask so CMake receives the prebuilt Rust
# bridge artifact. cargo flutter/dart are general passthrough commands only.
```

Riverpod、Freezed、l10n 和 FRB 生成必须从仓库根目录使用
`cargo xtask generate-gui`，不得手工修改生成文件。普通 `run-gui` 和 `build-gui` 只消费当前
源码，不执行生成器或可写格式化；修改生成输入后必须先显式生成。`cargo xtask check-gui-generated`
会重新生成并检查 Git 差异，适合提交前确认；CI/发布构建使用 `build-gui --check-generated`。
完整检查仍使用 `cargo xtask verify-gui`。xtask 会校验 codegen
版本，并在 Windows 上统一 FRB 2.12 用于 Rust crate 和输出的路径表示。

`tool/task_driver_harness.ps1` uses a separate `GuiStartupTimeoutSeconds`
(30 minutes by default) for the first Rust/Flutter build. Plan, Task, and stall
timeouts start only after the VM Service is available.

The default app path initializes the native FRB runtime and subscribes only to the selected session stream. `DemoStudioApi` is selected only by an explicit demo build flag or a test override; native runtime failures are surfaced instead of switching implementations.
