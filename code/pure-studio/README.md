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
cargo xtask run-gui
cargo xtask build-gui

# Run the native app through the dedicated test_driver entrypoint for Dart MCP
# interaction and GUI acceptance. The driver command owns the resident process
# tree and keeps Flutter's control pipe open. Release builds never use it.
cargo xtask run-gui --driver
cargo xtask run-gui --demo --driver # deterministic demo data

# Auxiliary commands may run from this Flutter project directory. Windows GUI
# run/build must use xtask so CMake receives the prebuilt Rust bridge artifact.
flutter pub get
flutter_rust_bridge_codegen generate
flutter analyze
flutter test
```

The default app path initializes the native FRB runtime and subscribes only to the selected session stream. `DemoStudioApi` is selected only by an explicit demo build flag or a test override; native runtime failures are surfaced instead of switching implementations.
