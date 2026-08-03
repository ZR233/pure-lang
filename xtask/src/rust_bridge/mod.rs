use crate::cli::{BridgeConfiguration, BuildRustBridgeOptions};
use crate::paths;
use crate::process;
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Stdio;

pub(crate) const BRIDGE_LIBRARY_ENV: &str = "PURE_STUDIO_BRIDGE_LIBRARY";
pub(crate) const BRIDGE_DEBUG_SYMBOLS_ENV: &str = "PURE_STUDIO_BRIDGE_DEBUG_SYMBOLS";

const BRIDGE_PACKAGE_NAME: &str = "pl-studio-bridge";
const BRIDGE_TARGET_NAME: &str = "pl_studio_bridge";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RustBridgeArtifacts {
    dynamic_library: PathBuf,
    debug_symbols: Option<PathBuf>,
}

impl RustBridgeArtifacts {
    pub(crate) fn dynamic_library(&self) -> &Path {
        &self.dynamic_library
    }

    pub(crate) fn debug_symbols(&self) -> Option<&Path> {
        self.debug_symbols.as_deref()
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        dynamic_library: impl Into<PathBuf>,
        debug_symbols: Option<PathBuf>,
    ) -> Self {
        Self {
            dynamic_library: dynamic_library.into(),
            debug_symbols,
        }
    }
}

pub(crate) fn build(options: BuildRustBridgeOptions) -> Result<()> {
    if !cfg!(target_os = "windows") {
        bail!("build-rust-bridge is only supported for Flutter Windows builds");
    }

    let workspace_root = resolve_workspace_root(&options.workspace_root)?;
    let artifacts = build_artifacts(
        &workspace_root,
        options.configuration,
        options.target_dir.as_deref(),
    )?;
    copy_artifacts(&artifacts, &options.output_dir)
}

pub(crate) fn build_workspace_artifacts(
    workspace_root: &Path,
    configuration: BridgeConfiguration,
) -> Result<RustBridgeArtifacts> {
    let workspace_root = resolve_workspace_root(workspace_root)?;
    build_artifacts(&workspace_root, configuration, None)
}

fn resolve_workspace_root(workspace_root: &Path) -> Result<PathBuf> {
    let workspace_root = fs::canonicalize(workspace_root).with_context(|| {
        format!(
            "failed to resolve workspace root {}",
            workspace_root.display()
        )
    })?;
    paths::ensure_workspace_shape(&workspace_root)?;
    Ok(workspace_root)
}

fn build_artifacts(
    workspace_root: &Path,
    configuration: BridgeConfiguration,
    target_dir: Option<&Path>,
) -> Result<RustBridgeArtifacts> {
    let args = cargo_build_args(configuration, target_dir);
    let display = process::display_command("cargo", &args);
    let mut command = process::path_command("cargo", &args);
    command
        .current_dir(workspace_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());

    println!("==> ({}) {display}", workspace_root.display());
    let mut child = command
        .spawn()
        .with_context(|| format!("failed to start command from PATH: {display}"))?;
    let stdout = child
        .stdout
        .take()
        .with_context(|| format!("failed to capture command stdout: {display}"))?;
    let artifacts = match parse_cargo_messages(BufReader::new(stdout)) {
        Ok(artifacts) => artifacts,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error).with_context(|| format!("failed to parse Cargo output: {display}"));
        }
    };
    let status = child
        .wait()
        .with_context(|| format!("failed to wait for command: {display}"))?;
    if !status.success() {
        let code = status
            .code()
            .map(|code| code.to_string())
            .unwrap_or_else(|| "terminated by signal".to_owned());
        bail!("command failed with exit code {code}: {display}");
    }

    validate_artifacts(artifacts)
}

fn cargo_build_args(
    configuration: BridgeConfiguration,
    target_dir: Option<&Path>,
) -> Vec<OsString> {
    let mut args = vec![
        OsString::from("build"),
        OsString::from("-p"),
        OsString::from(BRIDGE_PACKAGE_NAME),
        OsString::from("--message-format=json-render-diagnostics"),
    ];
    if configuration.uses_release_profile() {
        args.push(OsString::from("--release"));
    }
    if let Some(target_dir) = target_dir {
        args.push(OsString::from("--target-dir"));
        args.push(target_dir.as_os_str().to_owned());
    }
    args
}

fn parse_cargo_messages(reader: impl BufRead) -> Result<RustBridgeArtifacts> {
    let mut artifacts = None;
    for line in reader.lines() {
        let line = line.context("failed to read Cargo JSON message")?;
        let message: CargoMessage =
            serde_json::from_str(&line).context("Cargo emitted an invalid JSON message")?;
        if message.reason == "compiler-message" {
            if let Some(rendered) = message.message.and_then(|message| message.rendered) {
                eprint!("{rendered}");
            }
            continue;
        }
        if message.reason != "compiler-artifact" || !message.is_bridge_cdylib() {
            continue;
        }
        if artifacts.is_some() {
            bail!("Cargo emitted multiple compiler artifacts for {BRIDGE_TARGET_NAME}");
        }
        artifacts = Some(artifacts_from_filenames(message.filenames)?);
    }

    artifacts.with_context(|| {
        format!("Cargo did not emit a cdylib compiler artifact for {BRIDGE_TARGET_NAME}")
    })
}

fn artifacts_from_filenames(filenames: Vec<PathBuf>) -> Result<RustBridgeArtifacts> {
    let dynamic_library =
        select_unique_extension(&filenames, dynamic_library_extension(), "dynamic library")?
            .with_context(|| {
                format!(
                    "Cargo artifact for {BRIDGE_TARGET_NAME} did not include a .{} dynamic library",
                    dynamic_library_extension()
                )
            })?;
    let debug_symbols = debug_symbols_extension()
        .map(|extension| select_unique_extension(&filenames, extension, "debug symbols"))
        .transpose()?
        .flatten();

    Ok(RustBridgeArtifacts {
        dynamic_library,
        debug_symbols,
    })
}

fn select_unique_extension(
    filenames: &[PathBuf],
    extension: &str,
    artifact_kind: &str,
) -> Result<Option<PathBuf>> {
    let mut matches = filenames
        .iter()
        .filter(|path| path.extension() == Some(OsStr::new(extension)));
    let selected = matches.next().cloned();
    if matches.next().is_some() {
        bail!("Cargo artifact for {BRIDGE_TARGET_NAME} included multiple {artifact_kind} files");
    }
    Ok(selected)
}

fn validate_artifacts(artifacts: RustBridgeArtifacts) -> Result<RustBridgeArtifacts> {
    let dynamic_library = validate_artifact_path(&artifacts.dynamic_library, "dynamic library")?;
    let debug_symbols = artifacts
        .debug_symbols
        .as_deref()
        .map(|path| validate_artifact_path(path, "debug symbols"))
        .transpose()?;
    Ok(RustBridgeArtifacts {
        dynamic_library,
        debug_symbols,
    })
}

fn validate_artifact_path(path: &Path, artifact_kind: &str) -> Result<PathBuf> {
    if !path.is_absolute() {
        bail!(
            "Cargo emitted a non-absolute Rust bridge {artifact_kind} path: {}",
            path.display()
        );
    }
    if !path.is_file() {
        bail!(
            "Rust bridge {artifact_kind} was not produced: {}",
            path.display()
        );
    }
    Ok(path.to_path_buf())
}

fn copy_artifacts(artifacts: &RustBridgeArtifacts, output_dir: &Path) -> Result<()> {
    fs::create_dir_all(output_dir)
        .with_context(|| format!("failed to create {}", output_dir.display()))?;
    copy_artifact(artifacts.dynamic_library(), output_dir)?;
    if let Some(debug_symbols) = artifacts.debug_symbols() {
        copy_artifact(debug_symbols, output_dir)?;
    }
    Ok(())
}

fn copy_artifact(source: &Path, output_dir: &Path) -> Result<()> {
    let file_name = source.file_name().with_context(|| {
        format!(
            "Rust bridge artifact has no file name: {}",
            source.display()
        )
    })?;
    fs::copy(source, output_dir.join(file_name))
        .with_context(|| format!("failed to copy {}", source.display()))?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn dynamic_library_extension() -> &'static str {
    "dll"
}

#[cfg(target_os = "macos")]
fn dynamic_library_extension() -> &'static str {
    "dylib"
}

#[cfg(all(unix, not(target_os = "macos")))]
fn dynamic_library_extension() -> &'static str {
    "so"
}

#[cfg(target_os = "windows")]
fn debug_symbols_extension() -> Option<&'static str> {
    Some("pdb")
}

#[cfg(not(target_os = "windows"))]
fn debug_symbols_extension() -> Option<&'static str> {
    None
}

#[derive(Debug, Deserialize)]
struct CargoMessage {
    reason: String,
    #[serde(default)]
    target: Option<CargoTarget>,
    #[serde(default)]
    profile: Option<CargoProfile>,
    #[serde(default)]
    filenames: Vec<PathBuf>,
    #[serde(default)]
    message: Option<CargoCompilerMessage>,
}

impl CargoMessage {
    fn is_bridge_cdylib(&self) -> bool {
        self.target.as_ref().is_some_and(|target| {
            target.name == BRIDGE_TARGET_NAME
                && target.crate_types.iter().any(|kind| kind == "cdylib")
        }) && self.profile.as_ref().is_some_and(|profile| !profile.test)
    }
}

#[derive(Debug, Deserialize)]
struct CargoTarget {
    name: String,
    #[serde(default)]
    crate_types: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CargoProfile {
    test: bool,
}

#[derive(Debug, Deserialize)]
struct CargoCompilerMessage {
    rendered: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::io::Cursor;

    #[test]
    fn cargo_args_map_debug_and_release_profiles() {
        assert_eq!(
            cargo_build_args(BridgeConfiguration::Debug, None),
            vec![
                OsString::from("build"),
                OsString::from("-p"),
                OsString::from(BRIDGE_PACKAGE_NAME),
                OsString::from("--message-format=json-render-diagnostics"),
            ]
        );
        for configuration in [BridgeConfiguration::Profile, BridgeConfiguration::Release] {
            assert_eq!(
                cargo_build_args(configuration, Some(Path::new("custom-target"))),
                vec![
                    OsString::from("build"),
                    OsString::from("-p"),
                    OsString::from(BRIDGE_PACKAGE_NAME),
                    OsString::from("--message-format=json-render-diagnostics"),
                    OsString::from("--release"),
                    OsString::from("--target-dir"),
                    OsString::from("custom-target"),
                ]
            );
        }
    }

    #[test]
    fn parses_fresh_bridge_artifact_and_ignores_unrelated_targets() -> Result<()> {
        let library = format!(
            "target/debug/pl_studio_bridge.{}",
            dynamic_library_extension()
        );
        let debug_symbols = debug_symbols_extension()
            .map(|extension| format!("target/debug/pl_studio_bridge.{extension}"));
        let mut bridge_filenames = vec![library.as_str()];
        if let Some(debug_symbols) = debug_symbols.as_deref() {
            bridge_filenames.push(debug_symbols);
        }
        let input = format!(
            "{}\n{}\n",
            artifact_json("other_target", false, &[library.as_str()]),
            artifact_json(BRIDGE_TARGET_NAME, false, &bridge_filenames)
        );

        let artifacts = parse_cargo_messages(Cursor::new(input))?;

        assert_eq!(
            artifacts,
            RustBridgeArtifacts {
                dynamic_library: PathBuf::from(library),
                debug_symbols: debug_symbols.map(PathBuf::from),
            }
        );
        Ok(())
    }

    #[test]
    fn ignores_test_artifact_and_allows_missing_debug_symbols() -> Result<()> {
        let library = format!(
            "target/debug/pl_studio_bridge.{}",
            dynamic_library_extension()
        );
        let input = format!(
            "{}\n{}\n",
            artifact_json(BRIDGE_TARGET_NAME, true, &[library.as_str()]),
            artifact_json(BRIDGE_TARGET_NAME, false, &[library.as_str()])
        );

        let artifacts = parse_cargo_messages(Cursor::new(input))?;

        assert_eq!(artifacts.debug_symbols, None);
        Ok(())
    }

    #[test]
    fn rejects_missing_or_duplicate_dynamic_library() {
        let missing = artifact_json(BRIDGE_TARGET_NAME, false, &["target/debug/bridge.rlib"]);
        assert!(
            parse_cargo_messages(Cursor::new(missing))
                .expect_err("missing dynamic library must fail")
                .to_string()
                .contains("did not include")
        );

        let first = format!("target/debug/one.{}", dynamic_library_extension());
        let second = format!("target/debug/two.{}", dynamic_library_extension());
        let duplicate = artifact_json(
            BRIDGE_TARGET_NAME,
            false,
            &[first.as_str(), second.as_str()],
        );
        assert!(
            parse_cargo_messages(Cursor::new(duplicate))
                .expect_err("duplicate dynamic libraries must fail")
                .to_string()
                .contains("multiple dynamic library")
        );
    }

    #[test]
    fn rejects_duplicate_compiler_artifact_and_invalid_json() {
        let library = format!(
            "target/debug/pl_studio_bridge.{}",
            dynamic_library_extension()
        );
        let artifact = artifact_json(BRIDGE_TARGET_NAME, false, &[library.as_str()]);
        let duplicate = format!("{artifact}\n{artifact}\n");
        assert!(
            parse_cargo_messages(Cursor::new(duplicate))
                .expect_err("duplicate compiler artifacts must fail")
                .to_string()
                .contains("multiple compiler artifacts")
        );
        assert!(
            parse_cargo_messages(Cursor::new("not json"))
                .expect_err("invalid Cargo JSON must fail")
                .to_string()
                .contains("invalid JSON")
        );
    }

    fn artifact_json(target_name: &str, test: bool, filenames: &[&str]) -> String {
        serde_json::json!({
            "reason": "compiler-artifact",
            "target": {
                "name": target_name,
                "crate_types": ["cdylib", "rlib"]
            },
            "profile": { "test": test },
            "filenames": filenames,
            "fresh": true
        })
        .to_string()
    }
}
