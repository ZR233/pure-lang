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
flutter pub get
flutter_rust_bridge_codegen generate
flutter analyze
flutter test
flutter build windows
```

The default app path initializes the native FRB runtime and subscribes only to the selected session stream. `DemoStudioApi` remains available for widget tests and emergency fallback wiring, but production UI state is driven by `pl-studio-bridge`.
