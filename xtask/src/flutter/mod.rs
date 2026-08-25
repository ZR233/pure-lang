use crate::cli::{BridgeConfiguration, BuildGuiOptions, LogLevel, RunGuiOptions, VerifyGuiOptions};
use crate::paths;
use crate::process;
use crate::pubspec_lock::{self, LockfileChange};
use crate::rust_bridge::{self, BRIDGE_DEBUG_SYMBOLS_ENV, BRIDGE_LIBRARY_ENV, RustBridgeArtifacts};
use crate::studio_version;
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

mod codegen;

const PUB_FINGERPRINT_FILE: &str = "pure-xtask-pub.sha256";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DesktopTarget {
    Windows,
    Macos,
    Linux,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DemoMode {
    Native,
    Demo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DriverMode {
    Disabled,
    Enabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FlutterProcessMode {
    Batch,
    ResidentDriver,
}

#[derive(Debug, Clone, Copy)]
struct FlutterInvocation<'a> {
    demo_mode: DemoMode,
    process_mode: FlutterProcessMode,
    bridge_artifacts: Option<&'a RustBridgeArtifacts>,
    log_level: Option<LogLevel>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DistCleanMode {
    Clean,
    KeepExisting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GeneratedSourcesPolicy {
    UseCurrent,
    RegenerateAndCheck,
}

impl GeneratedSourcesPolicy {
    fn prepare(self, workspace_root: &Path, app_dir: &Path) -> Result<()> {
        match self {
            Self::UseCurrent => Ok(()),
            Self::RegenerateAndCheck => {
                codegen::check_gui_generated_sources(workspace_root, app_dir)
            }
        }
    }
}

impl DesktopTarget {
    fn current() -> Result<Self> {
        if cfg!(target_os = "windows") {
            Ok(Self::Windows)
        } else if cfg!(target_os = "macos") {
            Ok(Self::Macos)
        } else if cfg!(target_os = "linux") {
            Ok(Self::Linux)
        } else {
            bail!("unsupported desktop OS for Flutter GUI build")
        }
    }

    fn flutter_name(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Macos => "macos",
            Self::Linux => "linux",
        }
    }

    fn release_artifact_dir(self, app_dir: &Path) -> PathBuf {
        match self {
            Self::Windows => app_dir
                .join("build")
                .join("windows")
                .join("x64")
                .join("runner")
                .join("Release"),
            Self::Macos => app_dir
                .join("build")
                .join("macos")
                .join("Build")
                .join("Products")
                .join("Release"),
            Self::Linux => app_dir
                .join("build")
                .join("linux")
                .join("x64")
                .join("release")
                .join("bundle"),
        }
    }
}

pub(crate) fn generate_gui() -> Result<()> {
    codegen::generate_gui()
}

pub(crate) fn check_gui_generated() -> Result<()> {
    codegen::check_gui_generated()
}

pub(crate) fn verify_gui(options: VerifyGuiOptions) -> Result<()> {
    let workspace_root = paths::workspace_root()?;
    let app_dir = paths::studio_app_dir(&workspace_root);
    print_context(&workspace_root, &app_dir);
    GeneratedSourcesPolicy::RegenerateAndCheck.prepare(&workspace_root, &app_dir)?;
    run_tool("cargo", &["fmt", "--all", "--check"], &workspace_root)?;
    run_tool(
        "dart",
        &[
            "format",
            "--output=none",
            "--set-exit-if-changed",
            "lib",
            "test",
            "integration_test",
            "test_driver",
        ],
        &app_dir,
    )?;
    run_flutter(
        &workspace_root,
        &app_dir,
        &["analyze", "--no-pub"],
        DemoMode::Native,
    )?;
    run_flutter(
        &workspace_root,
        &app_dir,
        &["test", "--no-pub", "--exclude-tags", "visual"],
        DemoMode::Native,
    )?;
    run_tool(
        "cargo",
        &["test", "-p", "pl-studio-bridge"],
        &workspace_root,
    )?;
    if options.integration {
        run_gui_integration(&workspace_root, &app_dir, DesktopTarget::current()?)?;
    }
    Ok(())
}

fn run_gui_integration(workspace_root: &Path, app_dir: &Path, target: DesktopTarget) -> Result<()> {
    let args = integration_drive_args(target)?;
    if matches!(target, DesktopTarget::Linux) && !linux_graphical_session_available() {
        return run_linux_headless_flutter(workspace_root, app_dir, &args);
    }
    run_flutter(workspace_root, app_dir, &args, DemoMode::Demo)
}

fn integration_drive_args(target: DesktopTarget) -> Result<Vec<&'static str>> {
    let device = match target {
        DesktopTarget::Windows => "windows",
        DesktopTarget::Linux => "linux",
        DesktopTarget::Macos => {
            bail!("verify-gui --integration currently supports Windows and Linux")
        }
    };
    Ok(vec![
        "drive",
        "--driver",
        "test_driver/integration_test.dart",
        "--target",
        "integration_test/studio_smoke_test.dart",
        "-d",
        device,
        "--no-pub",
    ])
}

fn linux_graphical_session_available() -> bool {
    graphical_session_available(
        std::env::var_os("DISPLAY").as_deref(),
        std::env::var_os("WAYLAND_DISPLAY").as_deref(),
    )
}

fn graphical_session_available(
    display: Option<&std::ffi::OsStr>,
    wayland: Option<&std::ffi::OsStr>,
) -> bool {
    [display, wayland]
        .into_iter()
        .flatten()
        .any(|value| !value.is_empty())
}

fn run_linux_headless_flutter(workspace_root: &Path, app_dir: &Path, args: &[&str]) -> Result<()> {
    ensure_linux_headless_dependencies(program_exists_on_path("xvfb-run"))?;
    let flutter_args = flutter_args(args, DemoMode::Demo);
    let mut xvfb_args = vec![
        OsString::from("-a"),
        OsString::from("--server-args=-screen 0 1440x900x24 -nolisten tcp -noreset"),
        OsString::from("flutter"),
    ];
    xvfb_args.extend(flutter_args);
    let display = process::display_command("xvfb-run", &xvfb_args);
    let mut command = process::path_command("xvfb-run", &xvfb_args);
    command.current_dir(app_dir);
    configure_flutter_environment(
        &mut command,
        FlutterInvocation {
            demo_mode: DemoMode::Demo,
            process_mode: FlutterProcessMode::Batch,
            bridge_artifacts: None,
            log_level: None,
        },
    );
    command.env("GDK_BACKEND", "x11");
    command.env("LIBGL_ALWAYS_SOFTWARE", "1");
    process::run_checked(&mut command, &display).with_context(|| {
        format!(
            "headless Linux Flutter integration; workspace root: {}, Studio app dir: {}",
            workspace_root.display(),
            app_dir.display()
        )
    })
}

fn ensure_linux_headless_dependencies(xvfb_run_available: bool) -> Result<()> {
    if !xvfb_run_available {
        bail!(
            "headless Linux Flutter integration requires xvfb-run; install it with 'sudo apt-get install -y xvfb'"
        );
    }
    Ok(())
}

fn program_exists_on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path)
                .map(|directory| directory.join(program))
                .any(|candidate| candidate.is_file())
        })
        .unwrap_or(false)
}

fn run_tool(program: &'static str, args: &[&str], cwd: &Path) -> Result<()> {
    let args = args.iter().map(OsString::from).collect::<Vec<_>>();
    run_os_tool(program, &args, cwd)
}

fn run_os_tool(program: &'static str, args: &[OsString], cwd: &Path) -> Result<()> {
    let display = process::display_command(program, args);
    let mut command = process::path_command(program, args);
    command.current_dir(cwd);
    process::run_checked(&mut command, &display)
}

pub(crate) fn run_gui(options: RunGuiOptions) -> Result<()> {
    let workspace_root = paths::workspace_root()?;
    let app_dir = paths::studio_app_dir(&workspace_root);
    let target = DesktopTarget::current()?;
    let app_version = studio_version::read(&app_dir)?;
    let version_define = format!("--dart-define=PURE_STUDIO_VERSION={app_version}");
    print_context(&workspace_root, &app_dir);
    ensure_flutter_dependencies(&workspace_root, &app_dir)?;
    GeneratedSourcesPolicy::UseCurrent.prepare(&workspace_root, &app_dir)?;

    let demo_mode = if options.demo {
        DemoMode::Demo
    } else {
        DemoMode::Native
    };
    let driver_mode = if options.driver {
        DriverMode::Enabled
    } else {
        DriverMode::Disabled
    };
    let run_args = run_gui_args(target, &version_define, driver_mode);
    let process_mode = match driver_mode {
        DriverMode::Disabled => FlutterProcessMode::Batch,
        DriverMode::Enabled => FlutterProcessMode::ResidentDriver,
    };
    let bridge_artifacts = prepare_bridge_artifacts(
        &workspace_root,
        target,
        demo_mode,
        BridgeConfiguration::Debug,
    )?;
    run_flutter_with_process_mode(
        &workspace_root,
        &app_dir,
        &run_args,
        FlutterInvocation {
            demo_mode,
            process_mode,
            bridge_artifacts: bridge_artifacts.as_ref(),
            log_level: options.log_level,
        },
    )
}

fn run_gui_args(target: DesktopTarget, version_define: &str, driver_mode: DriverMode) -> Vec<&str> {
    let mut args = Vec::new();
    if matches!(driver_mode, DriverMode::Enabled) {
        args.push("--print-dtd");
    }
    args.extend([
        "run",
        "-d",
        target.flutter_name(),
        version_define,
        "--no-pub",
    ]);
    if matches!(driver_mode, DriverMode::Enabled) {
        args.extend([
            "-t",
            "test_driver/driver_main.dart",
            "--dart-define=PURE_STUDIO_DRIVER=true",
            "--disable-service-auth-codes",
            "--verbose",
        ]);
    }
    args
}

pub(crate) fn build_gui(options: BuildGuiOptions) -> Result<()> {
    build_gui_with_version(options, None)
}

pub(crate) fn build_gui_release(options: BuildGuiOptions, version: &str) -> Result<()> {
    build_gui_with_version(options, Some(version))
}

fn build_gui_with_version(options: BuildGuiOptions, release_version: Option<&str>) -> Result<()> {
    let workspace_root = paths::workspace_root()?;
    let app_dir = paths::studio_app_dir(&workspace_root);
    let dist_dir = paths::release_dist_dir(&workspace_root);
    let target = DesktopTarget::current()?;
    let app_version = studio_version::read(&app_dir)?;
    if release_version.is_some_and(|version| version != app_version.to_string()) {
        bail!("release version does not match pubspec.yaml version {app_version}");
    }
    print_context(&workspace_root, &app_dir);
    ensure_flutter_dependencies(&workspace_root, &app_dir)?;
    generated_sources_policy(options.check_generated).prepare(&workspace_root, &app_dir)?;

    let version_define = format!("--dart-define=PURE_STUDIO_VERSION={app_version}");
    let args = build_gui_args(target, &version_define, options.demo);
    let demo_mode = if options.demo {
        DemoMode::Demo
    } else {
        DemoMode::Native
    };
    let bridge_artifacts = prepare_bridge_artifacts(
        &workspace_root,
        target,
        demo_mode,
        BridgeConfiguration::Release,
    )?;
    run_flutter_with_process_mode(
        &workspace_root,
        &app_dir,
        &args,
        FlutterInvocation {
            demo_mode,
            process_mode: FlutterProcessMode::Batch,
            bridge_artifacts: bridge_artifacts.as_ref(),
            log_level: None,
        },
    )?;

    let clean_mode = if options.no_clean {
        DistCleanMode::KeepExisting
    } else {
        DistCleanMode::Clean
    };
    copy_release_artifacts(
        &target.release_artifact_dir(&app_dir),
        &dist_dir,
        clean_mode,
    )
}

fn generated_sources_policy(check_generated: bool) -> GeneratedSourcesPolicy {
    if check_generated {
        GeneratedSourcesPolicy::RegenerateAndCheck
    } else {
        GeneratedSourcesPolicy::UseCurrent
    }
}

fn build_gui_args(target: DesktopTarget, version_define: &str, demo: bool) -> Vec<&str> {
    let mut args = vec![
        "build",
        target.flutter_name(),
        "--release",
        version_define,
        "--no-pub",
    ];
    if demo {
        args.push("--dart-define=PURE_STUDIO_DEMO=true");
    }
    args
}

fn ensure_flutter_dependencies(workspace_root: &Path, app_dir: &Path) -> Result<()> {
    let hosted_url = match std::env::var("PUB_HOSTED_URL") {
        Ok(hosted_url) => Some(hosted_url),
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(_)) => {
            bail!("PUB_HOSTED_URL must contain valid Unicode")
        }
    };
    let fingerprint = flutter_dependency_fingerprint(app_dir, hosted_url.as_deref())?;
    if has_cached_flutter_dependencies(app_dir, &fingerprint)? {
        println!("Flutter dependencies unchanged; reusing .dart_tool package configuration.");
        return Ok(());
    }

    let lock_path = app_dir.join("pubspec.lock");
    let original_lock = pubspec_lock::read_optional(&lock_path)?;
    let resolution_result = (|| {
        prepare_pubspec_lock_for_active_hosted_url(&lock_path, hosted_url.as_deref())?;
        run_flutter(workspace_root, app_dir, &["pub", "get"], DemoMode::Native)?;
        match pubspec_lock::classify_change(&lock_path, original_lock.as_deref())? {
            LockfileChange::Unchanged => {}
            LockfileChange::HostedUrlsOnly => {
                println!(
                    "Restoring canonical pubspec.lock hosted URLs after dependency resolution."
                );
            }
        }
        Ok(())
    })();
    let restore_result =
        pubspec_lock::restore_canonical_optional(&lock_path, original_lock.as_deref());
    match (resolution_result, restore_result) {
        (Err(error), _) => return Err(error),
        (Ok(()), Err(error)) => return Err(error),
        (Ok(()), Ok(())) => {}
    }

    let stamp_path = flutter_dependency_stamp_path(app_dir);
    let stamp_dir = stamp_path
        .parent()
        .context("Flutter dependency stamp has no parent directory")?;
    fs::create_dir_all(stamp_dir)
        .with_context(|| format!("failed to create {}", stamp_dir.display()))?;
    fs::write(&stamp_path, format!("{fingerprint}\n"))
        .with_context(|| format!("failed to write {}", stamp_path.display()))
}

fn prepare_pubspec_lock_for_active_hosted_url(
    lock_path: &Path,
    hosted_url: Option<&str>,
) -> Result<()> {
    let Some(hosted_url) = hosted_url else {
        return Ok(());
    };

    // Pub treats the hosted URL as part of a package's source identity. Align the
    // temporary lockfile before resolution so switching mirrors does not upgrade
    // otherwise locked dependencies.
    pubspec_lock::rewrite_hosted_urls(lock_path, hosted_url)
}

fn flutter_dependency_fingerprint(app_dir: &Path, hosted_url: Option<&str>) -> Result<String> {
    let mut hasher = Sha256::new();
    for file_name in ["pubspec.yaml", "pubspec.lock", "pubspec_overrides.yaml"] {
        hasher.update(file_name.as_bytes());
        hasher.update([0]);
        match fs::read(app_dir.join(file_name)) {
            Ok(content) => hasher.update(content),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                hasher.update(b"<missing>")
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("failed to read {}", app_dir.join(file_name).display())
                });
            }
        }
        hasher.update([0]);
    }
    hasher.update(hosted_url.unwrap_or("<default-hosted-url>").as_bytes());
    Ok(format!("{:x}", hasher.finalize()))
}

fn has_cached_flutter_dependencies(app_dir: &Path, fingerprint: &str) -> Result<bool> {
    if !app_dir
        .join(".dart_tool")
        .join("package_config.json")
        .is_file()
    {
        return Ok(false);
    }
    match fs::read_to_string(flutter_dependency_stamp_path(app_dir)) {
        Ok(cached) => Ok(cached.trim() == fingerprint),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).context("failed to read Flutter dependency fingerprint"),
    }
}

fn flutter_dependency_stamp_path(app_dir: &Path) -> PathBuf {
    app_dir.join(".dart_tool").join(PUB_FINGERPRINT_FILE)
}

fn print_context(workspace_root: &Path, app_dir: &Path) {
    println!("Workspace root: {}", workspace_root.display());
    println!("Studio app dir: {}", app_dir.display());
}

fn prepare_bridge_artifacts(
    workspace_root: &Path,
    target: DesktopTarget,
    demo_mode: DemoMode,
    configuration: BridgeConfiguration,
) -> Result<Option<RustBridgeArtifacts>> {
    if needs_bridge_artifacts(target, demo_mode) {
        return rust_bridge::build_workspace_artifacts(workspace_root, configuration).map(Some);
    }
    Ok(None)
}

fn needs_bridge_artifacts(target: DesktopTarget, demo_mode: DemoMode) -> bool {
    // Native 模式在所有桌面平台都需要预构建桥库；Demo 模式纯 Dart 运行。
    matches!(demo_mode, DemoMode::Native)
        && matches!(
            target,
            DesktopTarget::Windows | DesktopTarget::Linux | DesktopTarget::Macos
        )
}

fn run_flutter(
    workspace_root: &Path,
    app_dir: &Path,
    args: &[&str],
    demo_mode: DemoMode,
) -> Result<()> {
    run_flutter_with_process_mode(
        workspace_root,
        app_dir,
        args,
        FlutterInvocation {
            demo_mode,
            process_mode: FlutterProcessMode::Batch,
            bridge_artifacts: None,
            log_level: None,
        },
    )
}

fn run_flutter_with_process_mode(
    workspace_root: &Path,
    app_dir: &Path,
    args: &[&str],
    invocation: FlutterInvocation<'_>,
) -> Result<()> {
    let args = flutter_args(args, invocation.demo_mode);
    let display = process::display_command("flutter", &args);
    let mut command = process::path_command("flutter", &args);
    command.current_dir(app_dir);
    configure_flutter_environment(&mut command, invocation);
    let result = match invocation.process_mode {
        FlutterProcessMode::Batch => process::run_checked(&mut command, &display),
        FlutterProcessMode::ResidentDriver => process::run_resident_checked(&mut command, &display),
    };
    result.with_context(|| {
        format!(
            "workspace root: {}, Studio app dir: {}",
            workspace_root.display(),
            app_dir.display()
        )
    })
}

fn configure_flutter_environment(command: &mut Command, invocation: FlutterInvocation<'_>) {
    command.env_remove(BRIDGE_LIBRARY_ENV);
    command.env_remove(BRIDGE_DEBUG_SYMBOLS_ENV);
    match invocation.demo_mode {
        DemoMode::Native => {
            command.env_remove("PURE_STUDIO_DEMO");
        }
        DemoMode::Demo => {
            command.env("PURE_STUDIO_DEMO", "true");
        }
    }
    match invocation.log_level {
        Some(log_level) => {
            command.env("PURE_STUDIO_LOG_LEVEL", log_level.as_str());
        }
        None => {
            command.env_remove("PURE_STUDIO_LOG_LEVEL");
        }
    }
    if let Some(artifacts) = invocation.bridge_artifacts {
        command.env(BRIDGE_LIBRARY_ENV, artifacts.dynamic_library());
        if let Some(debug_symbols) = artifacts.debug_symbols() {
            command.env(BRIDGE_DEBUG_SYMBOLS_ENV, debug_symbols);
        }
    }
}

fn flutter_args(args: &[&str], demo_mode: DemoMode) -> Vec<OsString> {
    let mut result = args.iter().map(OsString::from).collect::<Vec<_>>();
    if matches!(demo_mode, DemoMode::Demo) && !args.contains(&"--dart-define=PURE_STUDIO_DEMO=true")
    {
        result.push(OsString::from("--dart-define=PURE_STUDIO_DEMO=true"));
    }
    result
}

fn copy_release_artifacts(
    artifact_dir: &Path,
    dist_dir: &Path,
    clean_mode: DistCleanMode,
) -> Result<()> {
    if !artifact_dir.is_dir() {
        bail!(
            "build artifact directory not found: {}",
            artifact_dir.display()
        );
    }
    if matches!(clean_mode, DistCleanMode::Clean) && dist_dir.exists() {
        fs::remove_dir_all(dist_dir)
            .with_context(|| format!("failed to clean {}", dist_dir.display()))?;
    }
    fs::create_dir_all(dist_dir)
        .with_context(|| format!("failed to create {}", dist_dir.display()))?;
    copy_dir_contents(artifact_dir, dist_dir)?;

    println!();
    println!("Release build complete.");
    println!("Output: {}", dist_dir.display());
    let mut files = fs::read_dir(dist_dir)
        .with_context(|| format!("failed to read {}", dist_dir.display()))?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| entry.file_name())
        .collect::<Vec<_>>();
    files.sort();
    if !files.is_empty() {
        println!("Files:");
        for file in files {
            println!("  {}", file.to_string_lossy());
        }
    }
    Ok(())
}

fn copy_dir_contents(source: &Path, destination: &Path) -> Result<()> {
    for entry in
        fs::read_dir(source).with_context(|| format!("failed to read {}", source.display()))?
    {
        let entry =
            entry.with_context(|| format!("failed to read entry in {}", source.display()))?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if entry
            .file_type()
            .with_context(|| format!("failed to inspect {}", source_path.display()))?
            .is_dir()
        {
            fs::create_dir_all(&destination_path)
                .with_context(|| format!("failed to create {}", destination_path.display()))?;
            copy_dir_contents(&source_path, &destination_path)?;
        } else {
            fs::copy(&source_path, &destination_path).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    source_path.display(),
                    destination_path.display()
                )
            })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::ffi::OsStr;

    #[test]
    fn ordinary_gui_build_uses_current_generated_sources() {
        assert_eq!(
            generated_sources_policy(false),
            GeneratedSourcesPolicy::UseCurrent
        );
        assert_eq!(
            generated_sources_policy(true),
            GeneratedSourcesPolicy::RegenerateAndCheck
        );
    }

    #[test]
    fn demo_mode_adds_dart_define_once() {
        assert_eq!(
            flutter_args(&["run", "-d", "windows"], DemoMode::Demo),
            vec![
                OsString::from("run"),
                OsString::from("-d"),
                OsString::from("windows"),
                OsString::from("--dart-define=PURE_STUDIO_DEMO=true"),
            ]
        );
        assert_eq!(
            flutter_args(
                &["build", "windows", "--dart-define=PURE_STUDIO_DEMO=true"],
                DemoMode::Demo
            ),
            vec![
                OsString::from("build"),
                OsString::from("windows"),
                OsString::from("--dart-define=PURE_STUDIO_DEMO=true"),
            ]
        );
    }

    #[test]
    fn driver_mode_selects_dedicated_entrypoint() {
        let version_define = "--dart-define=PURE_STUDIO_VERSION=1.2.3";

        assert_eq!(
            run_gui_args(DesktopTarget::Windows, version_define, DriverMode::Enabled),
            vec![
                "--print-dtd",
                "run",
                "-d",
                "windows",
                version_define,
                "--no-pub",
                "-t",
                "test_driver/driver_main.dart",
                "--dart-define=PURE_STUDIO_DRIVER=true",
                "--disable-service-auth-codes",
                "--verbose",
            ]
        );
        assert_eq!(
            run_gui_args(DesktopTarget::Windows, version_define, DriverMode::Disabled,),
            vec!["run", "-d", "windows", version_define, "--no-pub"]
        );
    }

    #[test]
    fn integration_drive_targets_the_current_windows_or_linux_desktop() -> Result<()> {
        let expected_prefix = [
            "drive",
            "--driver",
            "test_driver/integration_test.dart",
            "--target",
            "integration_test/studio_smoke_test.dart",
            "-d",
        ];
        let windows = integration_drive_args(DesktopTarget::Windows)?;
        let linux = integration_drive_args(DesktopTarget::Linux)?;

        assert_eq!(&windows[..expected_prefix.len()], expected_prefix);
        assert_eq!(windows[expected_prefix.len()], "windows");
        assert_eq!(&linux[..expected_prefix.len()], expected_prefix);
        assert_eq!(linux[expected_prefix.len()], "linux");
        assert!(windows.contains(&"--no-pub"));
        assert!(linux.contains(&"--no-pub"));
        assert!(integration_drive_args(DesktopTarget::Macos).is_err());
        Ok(())
    }

    #[test]
    fn linux_graphical_session_accepts_x11_or_wayland_without_environment_mutation() {
        assert!(!graphical_session_available(None, None));
        assert!(!graphical_session_available(Some(OsStr::new("")), None));
        assert!(graphical_session_available(Some(OsStr::new(":1")), None));
        assert!(graphical_session_available(
            None,
            Some(OsStr::new("wayland-0"))
        ));
    }

    #[test]
    fn headless_linux_requires_xvfb_with_an_actionable_install_hint() -> Result<()> {
        ensure_linux_headless_dependencies(true)?;

        let error = ensure_linux_headless_dependencies(false)
            .expect_err("missing xvfb-run must reject headless integration");
        assert!(error.to_string().contains("sudo apt-get install -y xvfb"));
        Ok(())
    }

    #[test]
    fn release_build_never_references_driver_entrypoint() {
        let args = build_gui_args(
            DesktopTarget::Windows,
            "--dart-define=PURE_STUDIO_VERSION=1.2.3",
            false,
        );

        assert!(!args.contains(&"test_driver/driver_main.dart"));
        assert!(!args.contains(&"--dart-define=PURE_STUDIO_DRIVER=true"));
        assert!(!args.contains(&"--disable-service-auth-codes"));
        assert!(!args.contains(&"--no-dds"));
        assert!(!args.contains(&"--print-dtd"));
        assert!(!args.contains(&"--verbose"));
        assert!(!args.contains(&"-t"));
        assert!(args.contains(&"--no-pub"));
    }

    #[test]
    fn native_targets_require_the_bridge_and_demo_mode_does_not() {
        for target in [
            DesktopTarget::Windows,
            DesktopTarget::Linux,
            DesktopTarget::Macos,
        ] {
            assert!(needs_bridge_artifacts(target, DemoMode::Native));
            assert!(!needs_bridge_artifacts(target, DemoMode::Demo));
        }
    }

    #[test]
    fn flutter_environment_passes_native_bridge_artifacts() {
        let artifacts = RustBridgeArtifacts::for_test(
            PathBuf::from(r"C:\artifacts\pl_studio_bridge.dll"),
            Some(PathBuf::from(r"C:\artifacts\pl_studio_bridge.pdb")),
        );
        let mut command = Command::new("flutter");

        configure_flutter_environment(
            &mut command,
            FlutterInvocation {
                demo_mode: DemoMode::Native,
                process_mode: FlutterProcessMode::Batch,
                bridge_artifacts: Some(&artifacts),
                log_level: Some(LogLevel::Debug),
            },
        );

        assert_eq!(
            command_env(&command, BRIDGE_LIBRARY_ENV),
            Some(Some(OsString::from(r"C:\artifacts\pl_studio_bridge.dll")))
        );
        assert_eq!(
            command_env(&command, BRIDGE_DEBUG_SYMBOLS_ENV),
            Some(Some(OsString::from(r"C:\artifacts\pl_studio_bridge.pdb")))
        );
        assert_eq!(command_env(&command, "PURE_STUDIO_DEMO"), Some(None));
        assert_eq!(
            command_env(&command, "PURE_STUDIO_LOG_LEVEL"),
            Some(Some(OsString::from("debug")))
        );
    }

    #[test]
    fn flutter_environment_clears_bridge_artifacts_for_demo() {
        let mut command = Command::new("flutter");

        configure_flutter_environment(
            &mut command,
            FlutterInvocation {
                demo_mode: DemoMode::Demo,
                process_mode: FlutterProcessMode::ResidentDriver,
                bridge_artifacts: None,
                log_level: None,
            },
        );

        assert_eq!(command_env(&command, BRIDGE_LIBRARY_ENV), Some(None));
        assert_eq!(command_env(&command, BRIDGE_DEBUG_SYMBOLS_ENV), Some(None));
        assert_eq!(
            command_env(&command, "PURE_STUDIO_DEMO"),
            Some(Some(OsString::from("true")))
        );
        assert_eq!(command_env(&command, "PURE_STUDIO_LOG_LEVEL"), Some(None));
    }

    fn command_env(command: &Command, name: &str) -> Option<Option<OsString>> {
        command
            .get_envs()
            .find(|(key, _)| *key == OsStr::new(name))
            .map(|(_, value)| value.map(OsString::from))
    }

    #[test]
    fn dependency_fingerprint_invalidates_cached_package_configuration() -> Result<()> {
        let app_dir = std::env::temp_dir().join(format!(
            "pl-xtask-flutter-dependencies-{}",
            std::process::id()
        ));
        let dart_tool_dir = app_dir.join(".dart_tool");
        fs::create_dir_all(&dart_tool_dir)?;
        fs::write(app_dir.join("pubspec.yaml"), "name: fixture\n")?;
        fs::write(app_dir.join("pubspec.lock"), "packages: {}\n")?;
        fs::write(dart_tool_dir.join("package_config.json"), "{}\n")?;

        let fingerprint = flutter_dependency_fingerprint(&app_dir, None)?;
        assert!(!has_cached_flutter_dependencies(&app_dir, &fingerprint)?);
        fs::write(
            flutter_dependency_stamp_path(&app_dir),
            format!("{fingerprint}\n"),
        )?;
        assert!(has_cached_flutter_dependencies(&app_dir, &fingerprint)?);

        let mirror_fingerprint =
            flutter_dependency_fingerprint(&app_dir, Some("https://mirror.example"))?;
        assert_ne!(fingerprint, mirror_fingerprint);
        assert!(!has_cached_flutter_dependencies(
            &app_dir,
            &mirror_fingerprint
        )?);

        fs::write(app_dir.join("pubspec.yaml"), "name: changed_fixture\n")?;
        let changed_fingerprint = flutter_dependency_fingerprint(&app_dir, None)?;
        assert_ne!(fingerprint, changed_fingerprint);
        assert!(!has_cached_flutter_dependencies(
            &app_dir,
            &changed_fingerprint
        )?);

        fs::remove_dir_all(app_dir)?;
        Ok(())
    }
}
