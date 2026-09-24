use crate::paths;
use crate::process;
use anyhow::{Context, Result, bail};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) const BRIDGE_LIBRARY_ENV: &str = "ANYWORK_BRIDGE_LIBRARY";
pub(crate) const BRIDGE_DEBUG_SYMBOLS_ENV: &str = "ANYWORK_BRIDGE_DEBUG_SYMBOLS";

const BRIDGE_PACKAGE_NAME: &str = "pl-studio-bridge";
const BRIDGE_TARGET_NAME: &str = "pl_studio_bridge";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BridgeConfiguration {
    Debug,
    Profile,
    Release,
}

impl BridgeConfiguration {
    fn uses_release_profile(self) -> bool {
        matches!(self, Self::Profile | Self::Release)
    }
}

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
}

pub(crate) fn build_workspace_artifacts(
    workspace_root: &Path,
    configuration: BridgeConfiguration,
) -> Result<RustBridgeArtifacts> {
    let workspace_root = resolve_workspace_root(workspace_root)?;
    let artifact_target_dir = resolve_cargo_target_dir(
        &workspace_root,
        std::env::var_os("CARGO_TARGET_DIR").as_deref(),
    );
    let args = cargo_build_args(configuration);
    let display = process::display_command("cargo", &args);
    let mut command = process::path_command("cargo", &args);
    command.current_dir(&workspace_root);
    process::run_checked(&mut command, &display)?;

    locate_built_artifacts(&artifact_target_dir, configuration)
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

fn cargo_build_args(configuration: BridgeConfiguration) -> Vec<OsString> {
    let mut args = vec![
        OsString::from("build"),
        OsString::from("-p"),
        OsString::from(BRIDGE_PACKAGE_NAME),
        OsString::from("--features"),
        OsString::from("pl-studio-runtime/embedded-remote-helpers"),
    ];
    if configuration.uses_release_profile() {
        args.push(OsString::from("--release"));
    }
    args
}

fn resolve_cargo_target_dir(
    workspace_root: &Path,
    environment_target_dir: Option<&OsStr>,
) -> PathBuf {
    let configured_target_dir = environment_target_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target"));
    if configured_target_dir.is_absolute() {
        configured_target_dir
    } else {
        workspace_root.join(configured_target_dir)
    }
}

fn locate_built_artifacts(
    target_dir: &Path,
    configuration: BridgeConfiguration,
) -> Result<RustBridgeArtifacts> {
    let candidates = artifact_candidates(target_dir, configuration);
    let dynamic_library = validate_artifact_path(&candidates.dynamic_library, "dynamic library")?;
    let debug_symbols = candidates.debug_symbols.filter(|path| path.is_file());
    Ok(RustBridgeArtifacts {
        dynamic_library,
        debug_symbols,
    })
}

fn artifact_candidates(
    target_dir: &Path,
    configuration: BridgeConfiguration,
) -> RustBridgeArtifacts {
    let profile_dir = if configuration.uses_release_profile() {
        "release"
    } else {
        "debug"
    };
    let profile_dir = target_dir.join(profile_dir);
    RustBridgeArtifacts {
        dynamic_library: profile_dir.join(format!(
            "{}{BRIDGE_TARGET_NAME}.{}",
            dynamic_library_prefix(),
            dynamic_library_extension()
        )),
        debug_symbols: debug_symbols_extension()
            .map(|extension| profile_dir.join(format!("{BRIDGE_TARGET_NAME}.{extension}"))),
    }
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

#[cfg(target_os = "windows")]
fn dynamic_library_prefix() -> &'static str {
    ""
}

#[cfg(not(target_os = "windows"))]
fn dynamic_library_prefix() -> &'static str {
    "lib"
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
