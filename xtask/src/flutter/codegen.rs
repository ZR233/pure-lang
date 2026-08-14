use super::{
    DemoMode, ensure_flutter_dependencies, print_context, run_flutter, run_os_tool, run_tool,
};
use crate::{paths, process};
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const FRB_CODEGEN_VERSION: &str = "2.12.0";
const CODEGEN_FINGERPRINT_FILE: &str = "pure-xtask-codegen.sha256";
const CODEGEN_FINGERPRINT_VERSION: &str = "1";
// build_runner 2.7+ always removes conflicting outputs. The former
// --delete-conflicting-outputs option is removed and only emits a warning.
const BUILD_RUNNER_ARGS: &[&str] = &["run", "build_runner", "build"];
const CODEGEN_FINGERPRINT_PATHS: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    ":(glob)code/**/Cargo.toml",
    ":(glob)code/**/build.rs",
    ":(glob)code/**/src/**/*.rs",
    ":(glob)code/pure-studio/**/*.dart",
    ":(glob)code/pure-studio/**/*.arb",
    ":(glob)code/pure-studio/*.yaml",
    "code/pure-studio/pubspec.lock",
    "xtask/src/flutter/codegen.rs",
];
const GENERATED_OUTPUTS: &[GeneratedOutput] = &[
    GeneratedOutput::dart(
        ":(glob)code/pure-studio/lib/**/*.g.dart",
        GeneratedDartPath::FileSuffix(".g.dart"),
    ),
    GeneratedOutput::dart(
        ":(glob)code/pure-studio/lib/**/*.freezed.dart",
        GeneratedDartPath::FileSuffix(".freezed.dart"),
    ),
    GeneratedOutput::dart(
        ":(glob)code/pure-studio/lib/src/l10n/app_localizations*.dart",
        GeneratedDartPath::L10n,
    ),
    GeneratedOutput::dart(
        "code/pure-studio/lib/src/rust",
        GeneratedDartPath::Directory("src/rust"),
    ),
    GeneratedOutput::other("code/pure-studio/rust/src/frb_generated.rs"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GenerationPolicy {
    Always,
    WhenInputsChange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GeneratedDartPath {
    FileSuffix(&'static str),
    Directory(&'static str),
    L10n,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GeneratedOutput {
    git_pathspec: &'static str,
    dart_path: Option<GeneratedDartPath>,
}

impl GeneratedOutput {
    const fn dart(git_pathspec: &'static str, dart_path: GeneratedDartPath) -> Self {
        Self {
            git_pathspec,
            dart_path: Some(dart_path),
        }
    }

    const fn other(git_pathspec: &'static str) -> Self {
        Self {
            git_pathspec,
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

    generate_gui_sources(&workspace_root, &app_dir, GenerationPolicy::Always)
}

pub(super) fn check_gui_generated() -> Result<()> {
    let workspace_root = paths::workspace_root()?;
    let app_dir = paths::studio_app_dir(&workspace_root);
    print_context(&workspace_root, &app_dir);

    check_gui_generated_sources(&workspace_root, &app_dir)
}

pub(super) fn refresh_gui_generated_sources(workspace_root: &Path, app_dir: &Path) -> Result<()> {
    generate_gui_sources(workspace_root, app_dir, GenerationPolicy::WhenInputsChange)
}

pub(super) fn check_gui_generated_sources(workspace_root: &Path, app_dir: &Path) -> Result<()> {
    generate_gui_sources(workspace_root, app_dir, GenerationPolicy::Always)?;
    ensure_generated_files_are_committed(workspace_root)
}

pub(super) fn ensure_gui_generated_sources_are_committed(workspace_root: &Path) -> Result<()> {
    ensure_generated_files_are_committed(workspace_root)
}

fn generate_gui_sources(
    workspace_root: &Path,
    app_dir: &Path,
    policy: GenerationPolicy,
) -> Result<()> {
    if matches!(policy, GenerationPolicy::WhenInputsChange)
        && generated_sources_are_current(workspace_root, app_dir)?
    {
        println!("GUI codegen inputs unchanged; reusing canonical generated sources.");
        return Ok(());
    }

    ensure_flutter_dependencies(workspace_root, app_dir)?;
    run_flutter(workspace_root, app_dir, &["gen-l10n"], DemoMode::Native)?;
    ensure_frb_codegen_version()?;
    run_frb_codegen(workspace_root, app_dir)?;
    run_tool("dart", BUILD_RUNNER_ARGS, app_dir)?;
    normalize_generated_dart_whitespace(&app_dir.join("lib"))?;
    run_tool("dart", &["format", "lib"], app_dir)?;
    run_tool("cargo", &["fmt", "--all"], workspace_root)?;
    write_codegen_fingerprint(workspace_root, app_dir)
}

fn generated_sources_are_current(workspace_root: &Path, app_dir: &Path) -> Result<bool> {
    let cached = match fs::read_to_string(codegen_fingerprint_path(app_dir)) {
        Ok(cached) => cached,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error).context("failed to read GUI codegen fingerprint"),
    };
    Ok(cached.trim() == gui_codegen_fingerprint(workspace_root)?)
}

fn write_codegen_fingerprint(workspace_root: &Path, app_dir: &Path) -> Result<()> {
    let fingerprint = gui_codegen_fingerprint(workspace_root)?;
    let path = codegen_fingerprint_path(app_dir);
    let parent = path
        .parent()
        .context("GUI codegen fingerprint has no parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    fs::write(&path, format!("{fingerprint}\n"))
        .with_context(|| format!("failed to write {}", path.display()))
}

fn codegen_fingerprint_path(app_dir: &Path) -> PathBuf {
    app_dir.join(".dart_tool").join(CODEGEN_FINGERPRINT_FILE)
}

fn gui_codegen_fingerprint(workspace_root: &Path) -> Result<String> {
    let files = list_codegen_files(workspace_root)?;
    fingerprint_files(workspace_root, &files)
}

fn list_codegen_files(workspace_root: &Path) -> Result<Vec<String>> {
    let mut args = vec![
        OsString::from("ls-files"),
        OsString::from("--cached"),
        OsString::from("--others"),
        OsString::from("--exclude-standard"),
        OsString::from("-z"),
        OsString::from("--"),
    ];
    args.extend(CODEGEN_FINGERPRINT_PATHS.iter().map(OsString::from));
    let output = process::path_command("git", &args)
        .current_dir(workspace_root)
        .output()
        .context("failed to list GUI codegen inputs")?;
    if !output.status.success() {
        bail!("git failed while listing GUI codegen inputs");
    }
    let paths =
        String::from_utf8(output.stdout).context("GUI codegen input paths must be valid UTF-8")?;
    let mut files = paths
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    files.sort_unstable();
    files.dedup();
    Ok(files)
}

fn fingerprint_files(workspace_root: &Path, files: &[String]) -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(CODEGEN_FINGERPRINT_VERSION.as_bytes());
    hasher.update([0]);
    hasher.update(FRB_CODEGEN_VERSION.as_bytes());
    hasher.update([0]);
    for output in GENERATED_OUTPUTS {
        hasher.update(output.git_pathspec.as_bytes());
        hasher.update([0]);
    }
    for path in files {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        match fs::read(workspace_root.join(path)) {
            Ok(content) => hasher.update(content),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                hasher.update(b"<missing>")
            }
            Err(error) => {
                return Err(error).with_context(|| format!("failed to read codegen input {path}"));
            }
        }
        hasher.update([0]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn normalize_generated_dart_whitespace(root: &Path) -> Result<()> {
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
            let content = fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            let normalized = trim_trailing_horizontal_whitespace(&content);
            if normalized != content {
                fs::write(&path, normalized)
                    .with_context(|| format!("failed to normalize {}", path.display()))?;
            }
        }
    }
    Ok(())
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

fn trim_trailing_horizontal_whitespace(content: &str) -> String {
    let mut normalized = String::with_capacity(content.len());
    for chunk in content.split_inclusive('\n') {
        let (line, newline) = chunk
            .strip_suffix('\n')
            .map_or((chunk, ""), |line| (line, "\n"));
        let (line, carriage_return) = line
            .strip_suffix('\r')
            .map_or((line, ""), |line| (line, "\r"));
        normalized.push_str(line.trim_end_matches([' ', '\t']));
        normalized.push_str(carriage_return);
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

fn ensure_generated_files_are_committed(workspace_root: &Path) -> Result<()> {
    let mut diff_args = vec![
        OsString::from("diff"),
        OsString::from("--exit-code"),
        OsString::from("HEAD"),
    ];
    diff_args.push(OsString::from("--"));
    diff_args.extend(
        GENERATED_OUTPUTS
            .iter()
            .map(|output| OsString::from(output.git_pathspec)),
    );
    run_os_tool("git", &diff_args, workspace_root).context(
        "generated GUI sources are not canonical; do not edit generated files manually; commit the output from cargo xtask generate-gui",
    )?;

    let mut untracked_args = vec![
        OsString::from("ls-files"),
        OsString::from("--others"),
        OsString::from("--"),
    ];
    untracked_args.extend(
        GENERATED_OUTPUTS
            .iter()
            .map(|output| OsString::from(output.git_pathspec)),
    );
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
            "generated GUI sources are untracked; do not edit them manually; commit the output from cargo xtask generate-gui:\n{}",
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
    fn generated_whitespace_normalization_preserves_line_endings() {
        assert_eq!(
            trim_trailing_horizontal_whitespace("alpha  \r\nbeta\t\ngamma  "),
            "alpha\r\nbeta\ngamma"
        );
    }

    #[test]
    fn generated_whitespace_normalization_excludes_handwritten_dart() {
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
    fn generated_output_inventory_drives_git_checks_and_normalization() {
        assert_eq!(
            GENERATED_OUTPUTS
                .iter()
                .map(|output| output.git_pathspec)
                .collect::<Vec<_>>(),
            vec![
                ":(glob)code/pure-studio/lib/**/*.g.dart",
                ":(glob)code/pure-studio/lib/**/*.freezed.dart",
                ":(glob)code/pure-studio/lib/src/l10n/app_localizations*.dart",
                "code/pure-studio/lib/src/rust",
                "code/pure-studio/rust/src/frb_generated.rs",
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
    fn codegen_fingerprint_covers_source_and_generated_content() -> Result<()> {
        let root = std::env::temp_dir().join(format!(
            "pl-xtask-codegen-fingerprint-{}",
            std::process::id()
        ));
        fs::create_dir_all(&root)?;
        fs::write(root.join("source.dart"), "source\n")?;
        fs::write(root.join("source.g.dart"), "generated\n")?;
        let files = vec!["source.dart".to_owned(), "source.g.dart".to_owned()];

        let original = fingerprint_files(&root, &files)?;
        fs::write(root.join("source.dart"), "changed source\n")?;
        assert_ne!(original, fingerprint_files(&root, &files)?);
        fs::write(root.join("source.dart"), "source\n")?;
        fs::write(root.join("source.g.dart"), "manual edit\n")?;
        assert_ne!(original, fingerprint_files(&root, &files)?);

        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn codegen_fingerprint_inventory_includes_all_generator_boundaries() {
        assert!(CODEGEN_FINGERPRINT_PATHS.contains(&":(glob)code/pure-studio/**/*.dart"));
        assert!(CODEGEN_FINGERPRINT_PATHS.contains(&":(glob)code/pure-studio/**/*.arb"));
        assert!(CODEGEN_FINGERPRINT_PATHS.contains(&":(glob)code/**/src/**/*.rs"));
        assert!(CODEGEN_FINGERPRINT_PATHS.contains(&"xtask/src/flutter/codegen.rs"));
    }

    #[test]
    fn staged_generated_changes_do_not_pass_committed_check() -> Result<()> {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "pl-xtask-generated-git-check-{}-{unique}",
            std::process::id()
        ));
        let generated = root.join("code/pure-studio/lib/model.g.dart");
        fs::create_dir_all(
            generated
                .parent()
                .context("generated fixture has no parent")?,
        )?;
        fs::write(&generated, "// canonical\n")?;
        run_tool("git", &["init", "--quiet"], &root)?;
        run_tool("git", &["add", "."], &root)?;
        run_tool(
            "git",
            &[
                "-c",
                "user.name=Pure Xtask",
                "-c",
                "user.email=xtask@example.invalid",
                "commit",
                "--quiet",
                "-m",
                "fixture",
            ],
            &root,
        )?;

        fs::write(&generated, "// staged but not committed\n")?;
        run_tool("git", &["add", "."], &root)?;
        let error = ensure_generated_files_are_committed(&root)
            .expect_err("staged generated changes must still differ from HEAD");
        assert!(error.to_string().contains("not canonical"));

        run_tool(
            "git",
            &[
                "-c",
                "user.name=Pure Xtask",
                "-c",
                "user.email=xtask@example.invalid",
                "commit",
                "--quiet",
                "-m",
                "staged fixture",
            ],
            &root,
        )?;
        fs::write(root.join(".gitignore"), "ignored.freezed.dart\n")?;
        run_tool("git", &["add", ".gitignore"], &root)?;
        run_tool(
            "git",
            &[
                "-c",
                "user.name=Pure Xtask",
                "-c",
                "user.email=xtask@example.invalid",
                "commit",
                "--quiet",
                "-m",
                "ignore fixture",
            ],
            &root,
        )?;
        fs::write(
            root.join("code/pure-studio/lib/ignored.freezed.dart"),
            "// ignored generated output\n",
        )?;
        let error = ensure_generated_files_are_committed(&root)
            .expect_err("ignored generated outputs must still be reported as untracked");
        assert!(error.to_string().contains("untracked"));

        fs::remove_dir_all(root)?;
        Ok(())
    }
}
