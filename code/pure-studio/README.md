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
# interaction and GUI acceptance. Release builds never use it.
cargo xtask run-gui --driver

# Run from this Flutter project directory.
flutter pub get
flutter_rust_bridge_codegen generate
flutter analyze
flutter test
flutter build windows
```

The default app path initializes the native FRB runtime and subscribes only to the selected session stream. `DemoStudioApi` remains available for widget tests and emergency fallback wiring, but production UI state is driven by `pl-studio-bridge`.
