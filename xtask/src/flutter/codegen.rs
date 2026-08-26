use super::{
    DemoMode, ensure_flutter_dependencies, print_context, run_flutter, run_os_tool, run_tool,
};
use crate::{paths, pubspec_lock};
use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const FRB_CODEGEN_VERSION: &str = "2.12.0";
// build_runner 2.7+ always removes conflicting outputs. The former
// --delete-conflicting-outputs option is removed and only emits a warning.
const BUILD_RUNNER_ARGS: &[&str] = &["run", "build_runner", "build"];
const GENERATED_OUTPUTS: &[GeneratedOutput] = &[
    GeneratedOutput::dart(GeneratedDartPath::FileSuffix(".g.dart")),
    GeneratedOutput::dart(GeneratedDartPath::FileSuffix(".freezed.dart")),
    GeneratedOutput::dart(GeneratedDartPath::L10n),
    GeneratedOutput::dart(GeneratedDartPath::Directory("src/rust")),
    GeneratedOutput::other("code/pure-studio/rust/src/frb_generated.rs"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GeneratedDartPath {
    FileSuffix(&'static str),
    Directory(&'static str),
    L10n,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GeneratedOutput {
    workspace_path: Option<&'static str>,
    dart_path: Option<GeneratedDartPath>,
}

impl GeneratedOutput {
    const fn dart(dart_path: GeneratedDartPath) -> Self {
        Self {
            workspace_path: None,
            dart_path: Some(dart_path),
        }
    }

    const fn other(workspace_path: &'static str) -> Self {
        Self {
            workspace_path: Some(workspace_path),
            dart_path: None,
        }
    }

    fn matches_dart_path(self, relative: &Path, file_name: &str) -> bool {
        match self.dart_path {
            Some(GeneratedDartPath::FileSuffix(suffix)) => file_name.ends_with(suffix),
            Some(GeneratedDartPath::Directory(directory)) => {
                relative.starts_with(Path::new(directory))
                    && relative.extension().and_then(|value| value.to_str()) == Some("dart")
            }
            Some(GeneratedDartPath::L10n) => {
                relative.parent() == Some(Path::new("src").join("l10n").as_path())
                    && file_name.starts_with("app_localizations")
                    && relative.extension().and_then(|value| value.to_str()) == Some("dart")
            }
            None => false,
        }
    }
}

pub(super) fn generate_gui() -> Result<()> {
    let workspace_root = paths::workspace_root()?;
    let app_dir = paths::studio_app_dir(&workspace_root);
    print_context(&workspace_root, &app_dir);

    generate_gui_sources(&workspace_root, &app_dir)
}

pub(super) fn check_gui_generated() -> Result<()> {
    let workspace_root = paths::workspace_root()?;
    let app_dir = paths::studio_app_dir(&workspace_root);
    print_context(&workspace_root, &app_dir);

    check_gui_generated_sources(&workspace_root, &app_dir)
}

pub(super) fn check_gui_generated_sources(workspace_root: &Path, app_dir: &Path) -> Result<()> {
    let before = generated_sources_snapshot(workspace_root, app_dir)?;
    generate_gui_sources(workspace_root, app_dir)?;
    let after = generated_sources_snapshot(workspace_root, app_dir)?;
    ensure_generated_sources_are_stable(&before, &after)
}

fn generate_gui_sources(workspace_root: &Path, app_dir: &Path) -> Result<()> {
    ensure_flutter_dependencies(workspace_root, app_dir)?;
    run_flutter(workspace_root, app_dir, &["gen-l10n"], DemoMode::Native)?;
    ensure_frb_codegen_version()?;
    run_frb_codegen(workspace_root, app_dir)?;
    run_build_runner(app_dir)?;
    let generated_dart_files = normalize_generated_dart_files(&app_dir.join("lib"))?;
    format_generated_dart_files(app_dir, &generated_dart_files)?;
    format_generated_rust_file(workspace_root, app_dir)
}

fn run_build_runner(app_dir: &Path) -> Result<()> {
    let lock_path = app_dir.join("pubspec.lock");
    preserve_canonical_lockfile(&lock_path, || {
        match std::env::var("PUB_HOSTED_URL") {
            Ok(hosted_url) => {
                pubspec_lock::rewrite_hosted_urls(&lock_path, &hosted_url)?;
            }
            Err(std::env::VarError::NotPresent) => {}
            Err(std::env::VarError::NotUnicode(_)) => {
                anyhow::bail!("PUB_HOSTED_URL must contain valid Unicode")
            }
        }
        run_tool("dart", BUILD_RUNNER_ARGS, app_dir)
    })
}

fn preserve_canonical_lockfile(
    lock_path: &Path,
    operation: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let original_lock = pubspec_lock::read_optional(lock_path)?;
    let operation_result = operation();
    let validation_result = operation_result.as_ref().map_or(Ok(()), |_| {
        pubspec_lock::classify_change(lock_path, original_lock.as_deref()).map(|change| {
            if change == pubspec_lock::LockfileChange::HostedUrlsOnly {
                println!("Restoring canonical pubspec.lock hosted URLs after build_runner.");
            }
        })
    });
    let restore_result =
        pubspec_lock::restore_canonical_optional(lock_path, original_lock.as_deref());

    operation_result?;
    validation_result?;
    restore_result
}

fn normalize_generated_dart_files(root: &Path) -> Result<Vec<PathBuf>> {
    let files = generated_dart_files(root)?;
    for path in &files {
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let normalized = normalize_generated_dart_text(&content);
        if normalized != content {
            fs::write(path, normalized)
                .with_context(|| format!("failed to normalize {}", path.display()))?;
        }
    }
    Ok(files)
}

fn generated_dart_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(&directory)
            .with_context(|| format!("failed to read {}", directory.display()))?
        {
            let entry = entry
                .with_context(|| format!("failed to read entry in {}", directory.display()))?;
            let path = entry.path();
            if entry
                .file_type()
                .with_context(|| format!("failed to inspect {}", path.display()))?
                .is_dir()
            {
                directories.push(path);
                continue;
            }
            if !is_generated_dart_path(root, &path) {
                continue;
            }
            files.push(path);
        }
    }
    files.sort_unstable();
    Ok(files)
}

fn format_generated_dart_files(app_dir: &Path, files: &[PathBuf]) -> Result<()> {
    if files.is_empty() {
        return Ok(());
    }
    let mut args = vec![OsString::from("format")];
    for path in files {
        let relative = path.strip_prefix(app_dir).with_context(|| {
            format!(
                "generated Dart file {} is outside {}",
                path.display(),
                app_dir.display()
            )
        })?;
        args.push(relative.as_os_str().to_owned());
    }
    run_os_tool("dart", &args, app_dir)
}

fn format_generated_rust_file(workspace_root: &Path, app_dir: &Path) -> Result<()> {
    let generated = app_dir.join("rust").join("src").join("frb_generated.rs");
    let args = vec![
        OsString::from("--edition"),
        OsString::from("2024"),
        generated.into_os_string(),
    ];
    run_os_tool("rustfmt", &args, workspace_root)
}

fn is_generated_dart_path(root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    let Some(file_name) = relative.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    GENERATED_OUTPUTS
        .iter()
        .any(|output| output.matches_dart_path(relative, file_name))
}

fn normalize_generated_dart_text(content: &str) -> String {
    let mut normalized = String::with_capacity(content.len());
    for chunk in content.split_inclusive('\n') {
        let (line, newline) = chunk
            .strip_suffix('\n')
            .map_or((chunk, ""), |line| (line, "\n"));
        let line = line.strip_suffix('\r').unwrap_or(line);
        normalized.push_str(line.trim_end_matches([' ', '\t']));
        normalized.push_str(newline);
    }
    normalized
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

type GeneratedSourcesSnapshot = BTreeMap<PathBuf, Vec<u8>>;

fn generated_sources_snapshot(
    workspace_root: &Path,
    app_dir: &Path,
) -> Result<GeneratedSourcesSnapshot> {
    let mut snapshot = GeneratedSourcesSnapshot::new();
    for path in generated_dart_files(&app_dir.join("lib"))? {
        capture_generated_file(workspace_root, &path, &mut snapshot)?;
    }
    for workspace_path in GENERATED_OUTPUTS
        .iter()
        .filter_map(|output| output.workspace_path)
    {
        let path = workspace_root.join(workspace_path);
        if path
            .try_exists()
            .with_context(|| format!("failed to inspect generated source {}", path.display()))?
        {
            capture_generated_file(workspace_root, &path, &mut snapshot)?;
        }
    }
    Ok(snapshot)
}

fn capture_generated_file(
    workspace_root: &Path,
    path: &Path,
    snapshot: &mut GeneratedSourcesSnapshot,
) -> Result<()> {
    let relative = path.strip_prefix(workspace_root).with_context(|| {
        format!(
            "generated source {} is outside workspace {}",
            path.display(),
            workspace_root.display()
        )
    })?;
    let content = fs::read(path)
        .with_context(|| format!("failed to read generated source {}", path.display()))?;
    snapshot.insert(relative.to_path_buf(), content);
    Ok(())
}

fn ensure_generated_sources_are_stable(
    before: &GeneratedSourcesSnapshot,
    after: &GeneratedSourcesSnapshot,
) -> Result<()> {
    let mut changes = Vec::new();
    for (path, before_content) in before {
        match after.get(path) {
            Some(after_content) if before_content == after_content => {}
            Some(_) => changes.push(format!("modified: {}", canonical_workspace_path(path))),
            None => changes.push(format!("removed: {}", canonical_workspace_path(path))),
        }
    }
    for path in after.keys().filter(|path| !before.contains_key(*path)) {
        changes.push(format!("added: {}", canonical_workspace_path(path)));
    }
    if changes.is_empty() {
        return Ok(());
    }
    bail!(
        "generated GUI sources changed during canonical regeneration; run cargo xtask generate-gui and review the generated output before retrying:\n{}",
        changes.join("\n")
    )
}

fn canonical_workspace_path(path: &Path) -> String {
    path.iter()
        .map(|component| component.to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
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
                // execute the package because build_runner runs after FRB codegen.
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

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn frb_codegen_uses_windows_verbatim_paths_for_rust_inputs() {
        let path = Path::new(r"C:\workspace\code\pure-studio\rust");
        let expected = if cfg!(windows) {
            OsString::from(r"\\?\C:\workspace\code\pure-studio\rust")
        } else {
            path.as_os_str().to_os_string()
        };
        assert_eq!(codegen_rust_path(path), expected);
    }

    #[test]
    fn generated_text_normalization_uses_lf_and_trims_trailing_whitespace() {
        assert_eq!(
            normalize_generated_dart_text("alpha  \r\nbeta\t\ngamma  "),
            "alpha\nbeta\ngamma"
        );
    }

    #[test]
    fn generated_file_normalization_excludes_handwritten_dart() {
        let root = Path::new("lib");

        assert!(is_generated_dart_path(root, &root.join("models.g.dart")));
        assert!(is_generated_dart_path(
            root,
            &root.join("models.freezed.dart")
        ));
        assert!(is_generated_dart_path(
            root,
            &root.join("src/rust/frb_generated.dart")
        ));
        assert!(is_generated_dart_path(
            root,
            &root.join("src/l10n/app_localizations_en.dart")
        ));
        assert!(!is_generated_dart_path(
            root,
            &root.join("src/features/settings/editor.dart")
        ));
        assert!(!is_generated_dart_path(
            root,
            &root.join("src/l10n/studio_l10n.dart")
        ));
    }

    #[test]
    fn generated_output_inventory_drives_stability_checks_and_normalization() {
        assert_eq!(
            GENERATED_OUTPUTS,
            &[
                GeneratedOutput::dart(GeneratedDartPath::FileSuffix(".g.dart")),
                GeneratedOutput::dart(GeneratedDartPath::FileSuffix(".freezed.dart")),
                GeneratedOutput::dart(GeneratedDartPath::L10n),
                GeneratedOutput::dart(GeneratedDartPath::Directory("src/rust")),
                GeneratedOutput::other("code/pure-studio/rust/src/frb_generated.rs"),
            ]
        );
        assert!(GENERATED_OUTPUTS.iter().any(|output| {
            output.matches_dart_path(
                Path::new("src/data/repositories/studio_controller.g.dart"),
                "studio_controller.g.dart",
            )
        }));
        assert!(!GENERATED_OUTPUTS.iter().any(|output| {
            output.matches_dart_path(
                Path::new("src/data/repositories/studio_controller.dart"),
                "studio_controller.dart",
            )
        }));
    }

    #[test]
    fn build_runner_uses_current_conflicting_output_behavior() {
        assert_eq!(BUILD_RUNNER_ARGS, ["run", "build_runner", "build"]);
        assert!(!BUILD_RUNNER_ARGS.contains(&"--delete-conflicting-outputs"));
    }

    #[test]
    fn build_runner_guard_restores_canonical_hosted_urls() -> Result<()> {
        let lock_path = temporary_lock_path("mirror-restore");
        let canonical = hosted_lockfile("https://pub.dev", "2.13.0");
        fs::write(&lock_path, &canonical)?;

        preserve_canonical_lockfile(&lock_path, || {
            pubspec_lock::rewrite_hosted_urls(&lock_path, "https://mirror.example")
        })?;

        assert_eq!(fs::read_to_string(&lock_path)?, canonical);
        fs::remove_file(lock_path)?;
        Ok(())
    }

    #[test]
    fn build_runner_guard_rejects_dependency_drift_and_restores_lockfile() -> Result<()> {
        let lock_path = temporary_lock_path("dependency-drift");
        let canonical = hosted_lockfile("https://pub.dev", "2.13.0");
        fs::write(&lock_path, &canonical)?;

        let error = preserve_canonical_lockfile(&lock_path, || {
            fs::write(
                &lock_path,
                hosted_lockfile("https://mirror.example", "2.14.0"),
            )?;
            Ok(())
        })
        .expect_err("build_runner must not change dependency resolution");

        assert!(error.to_string().contains("beyond hosted source URLs"));
        assert_eq!(fs::read_to_string(&lock_path)?, canonical);
        fs::remove_file(lock_path)?;
        Ok(())
    }

    fn hosted_lockfile(url: &str, version: &str) -> String {
        format!(
            "packages:\n  async:\n    dependency: transitive\n    description:\n      name: async\n      url: \"{url}\"\n    source: hosted\n    version: \"{version}\"\n"
        )
    }

    fn temporary_lock_path(name: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time should be after Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "pl-xtask-codegen-{name}-{}-{unique}.lock",
            std::process::id()
        ))
    }

    #[test]
    fn generated_formatter_inventory_excludes_handwritten_dart() -> Result<()> {
        let root = std::env::temp_dir().join(format!(
            "pl-xtask-generated-format-inventory-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        let files = [
            "models.dart",
            "models.g.dart",
            "models.freezed.dart",
            "src/l10n/studio_l10n.dart",
            "src/l10n/app_localizations_en.dart",
            "src/rust/frb_generated.dart",
        ];
        for path in files {
            let path = root.join(path);
            fs::create_dir_all(path.parent().context("fixture has no parent")?)?;
            fs::write(path, "// fixture\n")?;
        }

        let actual = generated_dart_files(&root)?
            .into_iter()
            .map(|path| {
                path.strip_prefix(&root)
                    .expect("generated fixture must stay below its root")
                    .to_path_buf()
            })
            .collect::<Vec<_>>();

        assert_eq!(
            actual,
            vec![
                PathBuf::from("models.freezed.dart"),
                PathBuf::from("models.g.dart"),
                PathBuf::from("src/l10n/app_localizations_en.dart"),
                PathBuf::from("src/rust/frb_generated.dart"),
            ]
        );
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn uncommitted_generated_sources_pass_when_regeneration_is_stable() -> Result<()> {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "pl-xtask-generated-stability-check-{}-{unique}",
            std::process::id()
        ));
        let app_dir = root.join("code/pure-studio");
        let generated = app_dir.join("lib/model.g.dart");
        let ignored_generated = app_dir.join("lib/ignored.freezed.dart");
        let generated_rust = app_dir.join("rust/src/frb_generated.rs");
        for path in [&generated, &ignored_generated, &generated_rust] {
            fs::create_dir_all(path.parent().context("generated fixture has no parent")?)?;
            fs::write(path, "// uncommitted canonical output\n")?;
        }

        let before = generated_sources_snapshot(&root, &app_dir)?;
        let unchanged = generated_sources_snapshot(&root, &app_dir)?;
        ensure_generated_sources_are_stable(&before, &unchanged)?;

        fs::write(&generated, "// regenerated output\n")?;
        fs::remove_file(&ignored_generated)?;
        fs::write(app_dir.join("lib/new_model.g.dart"), "// new output\n")?;
        let changed = generated_sources_snapshot(&root, &app_dir)?;
        let error = ensure_generated_sources_are_stable(&before, &changed)
            .expect_err("regeneration changes must fail the stability check");
        let message = error.to_string();
        assert!(message.contains("modified: code/pure-studio/lib/model.g.dart"));
        assert!(message.contains("removed: code/pure-studio/lib/ignored.freezed.dart"));
        assert!(message.contains("added: code/pure-studio/lib/new_model.g.dart"));
        assert!(!message.contains("commit"));

        fs::remove_dir_all(root)?;
        Ok(())
    }
}
