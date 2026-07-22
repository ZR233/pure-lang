use crate::cli::{BuildGuiOptions, RunGuiOptions};
use crate::paths;
use crate::process;
use crate::pubspec_lock::{self, LockfileChange};
use crate::studio_version;
use anyhow::{Context, Result, bail};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

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

pub(crate) fn run_gui(options: RunGuiOptions) -> Result<()> {
    let workspace_root = paths::workspace_root()?;
    let app_dir = paths::flutter_app_dir(&workspace_root);
    let target = DesktopTarget::current()?;
    let app_version = studio_version::read(&app_dir)?.release;
    let version_define = format!("--dart-define=PURE_STUDIO_VERSION={app_version}");
    print_context(&workspace_root, &app_dir);

    run_flutter(&workspace_root, &app_dir, &["pub", "get"], DemoMode::Native)?;

    let demo_mode = if options.demo {
        DemoMode::Demo
    } else {
        DemoMode::Native
    };
    let run_result = run_flutter(
        &workspace_root,
        &app_dir,
        &["run", "-d", target.flutter_name(), &version_define],
        demo_mode,
    );

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
        return run_flutter(
            &workspace_root,
            &app_dir,
            &["run", "-d", target.flutter_name(), &version_define],
            DemoMode::Demo,
        );
    }

    run_result
}

pub(crate) fn build_gui(options: BuildGuiOptions) -> Result<()> {
    build_gui_with_version(options, None)
}

pub(crate) fn build_gui_release(options: BuildGuiOptions, version: &str) -> Result<()> {
    build_gui_with_version(options, Some(version))
}

fn build_gui_with_version(options: BuildGuiOptions, release_version: Option<&str>) -> Result<()> {
    let workspace_root = paths::workspace_root()?;
    let app_dir = paths::flutter_app_dir(&workspace_root);
    let dist_dir = paths::release_dist_dir(&workspace_root);
    let target = DesktopTarget::current()?;
    let app_version = studio_version::read(&app_dir)?.release;
    if release_version.is_some_and(|version| version != app_version.to_string()) {
        bail!("release version does not match pubspec.yaml version {app_version}");
    }
    let lock_path = app_dir.join("pubspec.lock");
    let original_lock = pubspec_lock::read_optional(&lock_path)?;
    print_context(&workspace_root, &app_dir);

    let build_result = (|| {
        run_flutter(&workspace_root, &app_dir, &["pub", "get"], DemoMode::Native)?;
        match pubspec_lock::classify_change(&lock_path, original_lock.as_deref())? {
            LockfileChange::Unchanged => {}
            LockfileChange::HostedUrlsOnly => {
                println!("pubspec.lock hosted URLs changed during pub get; restoring after build.");
            }
        }

        let mut args = vec!["build", target.flutter_name(), "--release"];
        let version_define = format!("--dart-define=PURE_STUDIO_VERSION={app_version}");
        args.push(&version_define);
        if options.demo {
            args.push("--dart-define=PURE_STUDIO_DEMO=true");
        }
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

fn print_context(workspace_root: &Path, app_dir: &Path) {
    println!("Workspace root: {}", workspace_root.display());
    println!("Flutter app dir: {}", app_dir.display());
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
            "workspace root: {}, Flutter app dir: {}",
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
}
