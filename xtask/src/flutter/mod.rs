use crate::cli::{BuildGuiOptions, RunGuiOptions, VerifyGuiOptions};
use crate::paths;
use crate::process;
use crate::pubspec_lock::{self, LockfileChange};
use crate::studio_version;
use anyhow::{Context, Result, bail};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const FRB_CODEGEN_VERSION: &str = "2.12.0";
const GENERATED_PATHS: &[&str] = &[
    ":(glob)code/pure-studio/lib/**/*.g.dart",
    ":(glob)code/pure-studio/lib/**/*.freezed.dart",
    ":(glob)code/pure-studio/lib/src/l10n/app_localizations*.dart",
    "code/pure-studio/lib/src/rust",
    "code/pure-studio/rust/src/frb_generated.rs",
];

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

    fn build_cache_dir(self, app_dir: &Path) -> PathBuf {
        app_dir.join("build").join(self.flutter_name())
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
    let workspace_root = paths::workspace_root()?;
    let app_dir = paths::studio_app_dir(&workspace_root);
    print_context(&workspace_root, &app_dir);

    run_flutter(&workspace_root, &app_dir, &["pub", "get"], DemoMode::Native)?;
    run_flutter(
        &workspace_root,
        &app_dir,
        &["pub", "run", "build_runner", "build"],
        DemoMode::Native,
    )?;
    run_flutter(&workspace_root, &app_dir, &["gen-l10n"], DemoMode::Native)?;
    ensure_frb_codegen_version()?;
    run_frb_codegen(&workspace_root, &app_dir)?;
    run_tool("dart", &["format", "lib"], &app_dir)?;
    run_tool("cargo", &["fmt", "--all"], &workspace_root)
}

pub(crate) fn verify_gui(options: VerifyGuiOptions) -> Result<()> {
    let workspace_root = paths::workspace_root()?;
    let app_dir = paths::studio_app_dir(&workspace_root);
    generate_gui()?;
    ensure_generated_files_are_committed(&workspace_root)?;
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
        if !cfg!(target_os = "windows") {
            bail!("verify-gui --integration currently requires Windows");
        }
        run_flutter(
            &workspace_root,
            &app_dir,
            &[
                "drive",
                "--driver",
                "test_driver/integration_test.dart",
                "--target",
                "integration_test/studio_smoke_test.dart",
                "-d",
                "windows",
            ],
            DemoMode::Demo,
        )?;
    }
    Ok(())
}

fn ensure_frb_codegen_version() -> Result<()> {
    let output = Command::new("flutter_rust_bridge_codegen")
        .arg("--version")
        .output()
        .context("failed to execute flutter_rust_bridge_codegen --version")?;
    if !output.status.success() {
        bail!("flutter_rust_bridge_codegen --version failed");
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let version = format!("{stdout}\n{stderr}");
    if !version
        .split_whitespace()
        .any(|token| token.trim_start_matches('v') == FRB_CODEGEN_VERSION)
    {
        bail!(
            "flutter_rust_bridge_codegen {FRB_CODEGEN_VERSION} is required; found {}",
            version.trim()
        );
    }
    Ok(())
}

fn ensure_generated_files_are_committed(workspace_root: &Path) -> Result<()> {
    let mut diff_args = vec![OsString::from("diff"), OsString::from("--exit-code")];
    diff_args.push(OsString::from("--"));
    diff_args.extend(GENERATED_PATHS.iter().map(OsString::from));
    run_os_tool("git", &diff_args, workspace_root)?;

    let mut untracked_args = vec![
        OsString::from("ls-files"),
        OsString::from("--others"),
        OsString::from("--exclude-standard"),
        OsString::from("--"),
    ];
    untracked_args.extend(GENERATED_PATHS.iter().map(OsString::from));
    let output = process::path_command("git", &untracked_args)
        .current_dir(workspace_root)
        .output()
        .context("failed to inspect untracked generated files")?;
    if !output.status.success() {
        bail!("git failed while inspecting untracked generated files");
    }
    let untracked = String::from_utf8_lossy(&output.stdout);
    if !untracked.trim().is_empty() {
        bail!(
            "generated files are untracked; run cargo xtask generate-gui and commit:\n{}",
            untracked.trim()
        );
    }
    Ok(())
}

fn run_frb_codegen(workspace_root: &Path, app_dir: &Path) -> Result<()> {
    let lock_path = app_dir.join("pubspec.lock");
    let original_lock =
        fs::read(&lock_path).with_context(|| format!("failed to read {}", lock_path.display()))?;
    let result = (|| {
        prepare_freezed_prerelease_for_frb(&lock_path)?;
        let rust_root = app_dir.join("rust");
        let rust_output = rust_root.join("src").join("frb_generated.rs");
        let dart_output = app_dir.join("lib").join("src").join("rust");
        let args = vec![
            OsString::from("generate"),
            OsString::from("--rust-root"),
            codegen_rust_path(&rust_root),
            OsString::from("--rust-input"),
            OsString::from("crate::api::studio"),
            OsString::from("--dart-root"),
            app_dir.as_os_str().to_os_string(),
            OsString::from("--dart-output"),
            dart_output.into_os_string(),
            OsString::from("--rust-output"),
            codegen_rust_path(&rust_output),
            OsString::from("--stop-on-error"),
            OsString::from("--no-build-runner"),
        ];
        run_os_tool("flutter_rust_bridge_codegen", &args, app_dir)
    })();
    let restore = fs::write(&lock_path, original_lock)
        .with_context(|| format!("failed to restore {}", lock_path.display()));
    match (result, restore) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), _) => Err(error).with_context(|| {
            format!(
                "workspace root: {}, Studio app dir: {}",
                workspace_root.display(),
                app_dir.display()
            )
        }),
        (Ok(()), Err(error)) => Err(error),
    }
}

fn codegen_rust_path(path: &Path) -> OsString {
    if cfg!(windows) {
        OsString::from(format!(r"\\?\{}", path.display()))
    } else {
        path.as_os_str().to_os_string()
    }
}

fn prepare_freezed_prerelease_for_frb(lock_path: &Path) -> Result<()> {
    let source = fs::read_to_string(lock_path)
        .with_context(|| format!("failed to read {}", lock_path.display()))?;
    let mut in_freezed = false;
    let mut output = String::with_capacity(source.len());
    for line in source.lines() {
        if line == "  freezed:" {
            in_freezed = true;
        } else if in_freezed && line.starts_with("  ") && !line.starts_with("    ") {
            in_freezed = false;
        }
        if in_freezed && line.trim_start().starts_with("version:") {
            let stable = line.split_once("-dev.").map_or(line, |(prefix, _)| {
                // FRB 2.12 rejects prerelease semver in pubspec.lock even though
                // the installed Freezed build is compatible. Codegen does not
                // execute the package because build_runner already ran above.
                prefix
            });
            output.push_str(stable);
            if stable != line {
                output.push('"');
            }
        } else {
            output.push_str(line);
        }
        output.push('\n');
    }
    fs::write(lock_path, output)
        .with_context(|| format!("failed to prepare {}", lock_path.display()))
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

    run_flutter(&workspace_root, &app_dir, &["pub", "get"], DemoMode::Native)?;

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
    let run_result = run_flutter(&workspace_root, &app_dir, &run_args, demo_mode);

    if run_result.is_err() && options.demo_fallback && !options.demo {
        eprintln!(
            "Native Studio run failed. Falling back to PURE_STUDIO_DEMO=true. Original error: {}",
            run_result.as_ref().expect_err("checked is_err")
        );
        let build_dir = target.build_cache_dir(&app_dir);
        if build_dir.exists() {
            fs::remove_dir_all(&build_dir)
                .with_context(|| format!("failed to remove {}", build_dir.display()))?;
        }
        return run_flutter(&workspace_root, &app_dir, &run_args, DemoMode::Demo);
    }

    run_result
}

fn run_gui_args(target: DesktopTarget, version_define: &str, driver_mode: DriverMode) -> Vec<&str> {
    let mut args = vec!["run", "-d", target.flutter_name(), version_define];
    if matches!(driver_mode, DriverMode::Enabled) {
        args.extend([
            "-t",
            "test_driver/driver_main.dart",
            "--dart-define=PURE_STUDIO_DRIVER=true",
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
    let lock_path = app_dir.join("pubspec.lock");
    let original_lock = pubspec_lock::read_optional(&lock_path)?;
    print_context(&workspace_root, &app_dir);

    let build_result = (|| {
        prepare_pubspec_lock_for_active_hosted_url(&lock_path)?;
        run_flutter(&workspace_root, &app_dir, &["pub", "get"], DemoMode::Native)?;
        match pubspec_lock::classify_change(&lock_path, original_lock.as_deref())? {
            LockfileChange::Unchanged => {}
            LockfileChange::HostedUrlsOnly => {
                println!("pubspec.lock hosted URLs changed during pub get; restoring after build.");
            }
        }

        let version_define = format!("--dart-define=PURE_STUDIO_VERSION={app_version}");
        let args = build_gui_args(target, &version_define, options.demo);
        let demo_mode = if options.demo {
            DemoMode::Demo
        } else {
            DemoMode::Native
        };
        run_flutter(&workspace_root, &app_dir, &args, demo_mode)?;

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
    })();

    let restore_result = pubspec_lock::restore_optional(&lock_path, original_lock.as_deref());
    match (build_result, restore_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(error),
    }
}

fn build_gui_args<'a>(target: DesktopTarget, version_define: &'a str, demo: bool) -> Vec<&'a str> {
    let mut args = vec!["build", target.flutter_name(), "--release", version_define];
    if demo {
        args.push("--dart-define=PURE_STUDIO_DEMO=true");
    }
    args
}

fn prepare_pubspec_lock_for_active_hosted_url(lock_path: &Path) -> Result<()> {
    let hosted_url = match std::env::var("PUB_HOSTED_URL") {
        Ok(hosted_url) => hosted_url,
        Err(std::env::VarError::NotPresent) => return Ok(()),
        Err(std::env::VarError::NotUnicode(_)) => {
            bail!("PUB_HOSTED_URL must contain valid Unicode")
        }
    };

    // Pub treats the hosted URL as part of a package's source identity. Align the
    // temporary lockfile before resolution so switching mirrors does not upgrade
    // otherwise locked dependencies.
    pubspec_lock::rewrite_hosted_urls(lock_path, &hosted_url)
}

fn print_context(workspace_root: &Path, app_dir: &Path) {
    println!("Workspace root: {}", workspace_root.display());
    println!("Studio app dir: {}", app_dir.display());
}

fn run_flutter(
    workspace_root: &Path,
    app_dir: &Path,
    args: &[&str],
    demo_mode: DemoMode,
) -> Result<()> {
    let args = flutter_args(args, demo_mode);
    let display = process::display_command("flutter", &args);
    let mut command = process::path_command("flutter", &args);
    command.current_dir(app_dir);
    match demo_mode {
        DemoMode::Native => {
            command.env_remove("PURE_STUDIO_DEMO");
        }
        DemoMode::Demo => {
            command.env("PURE_STUDIO_DEMO", "true");
        }
    }
    process::run_checked(&mut command, &display).with_context(|| {
        format!(
            "workspace root: {}, Studio app dir: {}",
            workspace_root.display(),
            app_dir.display()
        )
    })
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
                "run",
                "-d",
                "windows",
                version_define,
                "-t",
                "test_driver/driver_main.dart",
                "--dart-define=PURE_STUDIO_DRIVER=true",
            ]
        );
        assert_eq!(
            run_gui_args(DesktopTarget::Windows, version_define, DriverMode::Disabled,),
            vec!["run", "-d", "windows", version_define]
        );
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
        assert!(!args.contains(&"-t"));
    }
}
