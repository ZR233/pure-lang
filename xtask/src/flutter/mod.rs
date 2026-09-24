use crate::cli::{BuildGuiOptions, LogLevel, RunGuiOptions};
use crate::paths;
use crate::process;
use crate::pubspec_lock::{self, LockfileChange};
use crate::remote_helper;
use crate::rust_bridge::{
    self, BRIDGE_DEBUG_SYMBOLS_ENV, BRIDGE_LIBRARY_ENV, BridgeConfiguration, RustBridgeArtifacts,
};
use crate::studio_version;
use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

mod codegen;
#[cfg(target_os = "linux")]
mod linux;

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

pub(crate) fn verify_gui() -> Result<()> {
    let workspace_root = paths::workspace_root()?;
    let app_dir = paths::studio_app_dir(&workspace_root);
    print_context(&workspace_root, &app_dir);
    codegen::check_gui_generated_sources(&workspace_root, &app_dir)?;
    run_tool("cargo", &["fmt", "--all", "--check"], &workspace_root)?;
    run_tool(
        "dart",
        &[
            "format",
            "--output=none",
            "--set-exit-if-changed",
            "lib",
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
    Ok(())
}

fn ensure_desktop_build_environment(target: DesktopTarget) -> Result<()> {
    match target {
        DesktopTarget::Linux => {
            #[cfg(target_os = "linux")]
            {
                linux::ensure_native_build_environment()
            }
            #[cfg(not(target_os = "linux"))]
            {
                Ok(())
            }
        }
        DesktopTarget::Windows | DesktopTarget::Macos => Ok(()),
    }
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
    let version_define = format!("--dart-define=ANYWORK_VERSION={app_version}");
    print_context(&workspace_root, &app_dir);
    let preflight_started = std::time::Instant::now();
    ensure_desktop_build_environment(target)?;
    ensure_flutter_dependencies(&workspace_root, &app_dir)?;
    println!(
        "startup_stage=development_preflight elapsed_ms={}",
        preflight_started.elapsed().as_millis()
    );
    let demo_mode = if options.demo {
        DemoMode::Demo
    } else {
        DemoMode::Native
    };
    prepare_remote_helpers(&workspace_root, demo_mode)?;
    let driver_attachment_define = options
        .driver_attachment
        .as_ref()
        .map(|path| {
            let path = path
                .canonicalize()
                .with_context(|| format!("Driver attachment does not exist: {}", path.display()))?;
            ensure!(
                path.is_file(),
                "Driver attachment is not a file: {}",
                path.display()
            );
            Ok::<_, anyhow::Error>(format!(
                "--dart-define=ANYWORK_DRIVER_ATTACHMENT_PATH={}",
                path.to_string_lossy()
            ))
        })
        .transpose()?;
    let run_args = run_gui_args(
        target,
        &version_define,
        options.driver,
        options.profile,
        driver_attachment_define.as_deref(),
    );
    let process_mode = if options.driver {
        FlutterProcessMode::ResidentDriver
    } else {
        FlutterProcessMode::Batch
    };
    let bridge_artifacts = prepare_bridge_artifacts(
        &workspace_root,
        demo_mode,
        if options.profile {
            BridgeConfiguration::Profile
        } else {
            BridgeConfiguration::Debug
        },
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

fn run_gui_args<'a>(
    target: DesktopTarget,
    version_define: &'a str,
    driver: bool,
    profile: bool,
    driver_attachment_define: Option<&'a str>,
) -> Vec<&'a str> {
    let mut args = Vec::new();
    if driver {
        args.push("--print-dtd");
    }
    args.extend([
        "run",
        "-d",
        target.flutter_name(),
        version_define,
        "--no-pub",
    ]);
    if profile {
        args.push("--profile");
    }
    if driver {
        args.extend([
            "-t",
            "test_driver/driver_main.dart",
            "--dart-define=ANYWORK_DRIVER=true",
        ]);
        if let Some(define) = driver_attachment_define {
            args.push(define);
        }
        args.extend(["--disable-service-auth-codes", "--verbose"]);
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
    ensure_desktop_build_environment(target)?;
    ensure_flutter_dependencies(&workspace_root, &app_dir)?;
    if options.check_generated {
        codegen::check_gui_generated_sources(&workspace_root, &app_dir)?;
    }
    let version_define = format!("--dart-define=ANYWORK_VERSION={app_version}");
    let demo_mode = if options.demo {
        DemoMode::Demo
    } else {
        DemoMode::Native
    };
    prepare_remote_helpers(&workspace_root, demo_mode)?;
    let args = build_gui_args(target, &version_define);
    let bridge_artifacts =
        prepare_bridge_artifacts(&workspace_root, demo_mode, BridgeConfiguration::Release)?;
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

    let artifact_dir = target.release_artifact_dir(&app_dir);

    let clean_mode = if options.no_clean {
        DistCleanMode::KeepExisting
    } else {
        DistCleanMode::Clean
    };
    copy_release_artifacts(&artifact_dir, &dist_dir, clean_mode)
}

fn build_gui_args(target: DesktopTarget, version_define: &str) -> Vec<&str> {
    vec![
        "build",
        target.flutter_name(),
        "--release",
        version_define,
        "--no-pub",
    ]
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
        if let Some(hosted_url) = hosted_url.as_deref() {
            // Pub treats the hosted URL as part of a package's source identity.
            pubspec_lock::rewrite_hosted_urls(&lock_path, hosted_url)?;
        }
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
    Ok(hex::encode(hasher.finalize()))
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

fn prepare_remote_helpers(workspace_root: &Path, demo_mode: DemoMode) -> Result<()> {
    if matches!(demo_mode, DemoMode::Native) {
        let started = std::time::Instant::now();
        remote_helper::prepare_for_embedding(workspace_root)?;
        println!(
            "startup_stage=build_remote_helpers elapsed_ms={}",
            started.elapsed().as_millis()
        );
    }
    Ok(())
}

fn prepare_bridge_artifacts(
    workspace_root: &Path,
    demo_mode: DemoMode,
    configuration: BridgeConfiguration,
) -> Result<Option<RustBridgeArtifacts>> {
    if matches!(demo_mode, DemoMode::Native) {
        let started = std::time::Instant::now();
        let result =
            rust_bridge::build_workspace_artifacts(workspace_root, configuration).map(Some);
        println!(
            "startup_stage=build_bridge elapsed_ms={}",
            started.elapsed().as_millis()
        );
        return result;
    }
    Ok(None)
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
            command.env_remove("ANYWORK_DEMO");
        }
        DemoMode::Demo => {
            command.env("ANYWORK_DEMO", "true");
        }
    }
    match invocation.log_level {
        Some(log_level) => {
            command.env("ANYWORK_LOG_LEVEL", log_level.as_str());
        }
        None => {
            command.env_remove("ANYWORK_LOG_LEVEL");
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
    if matches!(demo_mode, DemoMode::Demo) && !args.contains(&"--dart-define=ANYWORK_DEMO=true") {
        result.push(OsString::from("--dart-define=ANYWORK_DEMO=true"));
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
        clean_directory_contents(dist_dir)?;
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

fn clean_directory_contents(directory: &Path) -> Result<()> {
    for entry in fs::read_dir(directory)
        .with_context(|| format!("failed to read {} for cleanup", directory.display()))?
    {
        let entry =
            entry.with_context(|| format!("failed to read entry in {}", directory.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to inspect {}", path.display()))?;
        let result = if file_type.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
        result.with_context(|| format!("failed to remove stale artifact {}", path.display()))?;
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
