use crate::cli::BuildRustBridgeOptions;
use crate::paths;
use crate::process;
use anyhow::{Context, Result, bail};
use std::ffi::OsString;
use std::fs;

pub(crate) fn build(options: BuildRustBridgeOptions) -> Result<()> {
    if !cfg!(target_os = "windows") {
        bail!("build-rust-bridge is only supported for Flutter Windows CMake builds");
    }

    let workspace_root = fs::canonicalize(&options.workspace_root).with_context(|| {
        format!(
            "failed to resolve workspace root {}",
            options.workspace_root.display()
        )
    })?;
    paths::ensure_workspace_shape(&workspace_root)?;

    let target_dir = options.target_dir.unwrap_or_else(|| {
        workspace_root
            .join("code")
            .join("pure-studio")
            .join("build")
            .join("rust-target")
    });

    let mut args = vec![
        OsString::from("build"),
        OsString::from("-p"),
        OsString::from("pl-studio-bridge"),
    ];
    if options.configuration.uses_release_profile() {
        args.push(OsString::from("--release"));
    }

    let display = process::display_command("cargo", &args);
    let mut command = process::path_command("cargo", &args);
    command.current_dir(&workspace_root);
    command.env("CARGO_TARGET_DIR", &target_dir);
    process::run_checked(&mut command, &display)?;

    let profile_dir = target_dir.join(options.configuration.profile_dir());
    let bridge_dll = profile_dir.join("pl_studio_bridge.dll");
    if !bridge_dll.is_file() {
        bail!("Rust bridge DLL was not produced: {}", bridge_dll.display());
    }

    fs::create_dir_all(&options.output_dir)
        .with_context(|| format!("failed to create {}", options.output_dir.display()))?;
    fs::copy(&bridge_dll, options.output_dir.join("pl_studio_bridge.dll"))
        .with_context(|| format!("failed to copy {}", bridge_dll.display()))?;

    let bridge_pdb = profile_dir.join("pl_studio_bridge.pdb");
    if bridge_pdb.is_file() {
        fs::copy(&bridge_pdb, options.output_dir.join("pl_studio_bridge.pdb"))
            .with_context(|| format!("failed to copy {}", bridge_pdb.display()))?;
    }

    Ok(())
}
