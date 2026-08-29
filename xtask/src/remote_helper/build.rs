use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

use super::{HELPER_FILE_NAME, SUPPORTED_TARGETS, local_helper_path};
use crate::cli::BuildRemoteHelperOptions;
use crate::paths;
use crate::process::run_checked;

const BUILDER_ENV: &str = "PURE_REMOTE_HELPER_BUILDER";
const CARGO_ZIGBUILD_EXECUTABLE: &str = "cargo-zigbuild";
const ZIG_EXECUTABLE: &str = "zig";
const CARGO_ZIGBUILD_ZIG_PATH_ENV: &str = "CARGO_ZIGBUILD_ZIG_PATH";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CargoBuilder {
    Cargo,
    Zigbuild,
}

pub(super) fn build(options: BuildRemoteHelperOptions) -> Result<()> {
    let targets = selected_targets(&options)?;
    let workspace_root = paths::workspace_root()?;
    build_targets(&workspace_root, &targets)
}

pub(super) fn build_targets(workspace_root: &Path, targets: &[&str]) -> Result<()> {
    let builder = cargo_builder()?;
    for target in targets {
        build_target(workspace_root, target, builder)?;
    }
    Ok(())
}

fn selected_targets(options: &BuildRemoteHelperOptions) -> Result<Vec<&str>> {
    if options.all_targets {
        return Ok(SUPPORTED_TARGETS.to_vec());
    }
    let target = options
        .target
        .as_deref()
        .context("build-remote-helper requires either --target <TARGET> or --all-targets")?;
    if !SUPPORTED_TARGETS.contains(&target) {
        bail!(
            "unsupported remote helper target '{target}'; supported targets: {}",
            SUPPORTED_TARGETS.join(", ")
        );
    }
    Ok(vec![target])
}

fn cargo_builder() -> Result<CargoBuilder> {
    match std::env::var(BUILDER_ENV) {
        Err(std::env::VarError::NotPresent) => Ok(default_builder()),
        Ok(value) if value.is_empty() || value == "cargo" => Ok(CargoBuilder::Cargo),
        Ok(value) if value == "zigbuild" => {
            ensure_zigbuild_available()?;
            Ok(CargoBuilder::Zigbuild)
        }
        Ok(value) => bail!("{BUILDER_ENV} must be 'cargo' or 'zigbuild', got '{value}'"),
        Err(error) => Err(error).context(format!("failed to read {BUILDER_ENV}")),
    }
}

fn default_builder() -> CargoBuilder {
    let builder = choose_default_builder(
        which::which(CARGO_ZIGBUILD_EXECUTABLE).is_ok(),
        zig_available(),
    );
    if matches!(builder, CargoBuilder::Zigbuild) {
        println!(concat!(
            "检测到 Zig 与 cargo-zigbuild，远程助手将自动使用 cargo zigbuild；",
            "如需强制使用系统交叉链接器，请设置 PURE_REMOTE_HELPER_BUILDER=cargo。"
        ));
    }
    builder
}

fn choose_default_builder(cargo_zigbuild: bool, zig: bool) -> CargoBuilder {
    if cargo_zigbuild && zig {
        CargoBuilder::Zigbuild
    } else {
        CargoBuilder::Cargo
    }
}

fn zig_available() -> bool {
    std::env::var_os(CARGO_ZIGBUILD_ZIG_PATH_ENV)
        .map(|path| Path::new(&path).is_file())
        .unwrap_or_else(|| which::which(ZIG_EXECUTABLE).is_ok())
}

fn ensure_zigbuild_available() -> Result<()> {
    let cargo_zigbuild = which::which(CARGO_ZIGBUILD_EXECUTABLE).is_ok();
    let zig = zig_available();
    if cargo_zigbuild && zig {
        return Ok(());
    }

    let mut missing = Vec::new();
    if !zig {
        missing.push("zig");
    }
    if !cargo_zigbuild {
        missing.push("cargo-zigbuild");
    }
    bail!(
        concat!(
            "PURE_REMOTE_HELPER_BUILDER=zigbuild 需要 {}。Windows 可执行 `winget install zig.zig`，",
            "随后执行 `cargo install cargo-zigbuild --locked`；完成后重新运行本命令。"
        ),
        missing.join("、")
    )
}

fn build_target(workspace_root: &Path, target: &str, builder: CargoBuilder) -> Result<()> {
    let mut command = Command::new("cargo");
    if matches!(builder, CargoBuilder::Zigbuild) {
        command.arg("zigbuild");
    } else {
        command.arg("build");
        let linker = discover_linker(target)?;
        command.env(linker_env_name(target), linker);
    }
    command
        .current_dir(workspace_root)
        .args([
            "--release",
            "--package",
            "pl-remote-helper",
            "--target",
            target,
        ])
        .env("CARGO_PROFILE_RELEASE_STRIP", "symbols")
        .env("CARGO_PROFILE_RELEASE_LTO", "thin")
        .env("CARGO_PROFILE_RELEASE_CODEGEN_UNITS", "1")
        .env("CARGO_PROFILE_RELEASE_PANIC", "abort");
    let display = match builder {
        CargoBuilder::Cargo => {
            format!("cargo build --release -p pl-remote-helper --target {target}")
        }
        CargoBuilder::Zigbuild => {
            format!("cargo zigbuild --release -p pl-remote-helper --target {target}")
        }
    };
    run_checked(&mut command, &display)?;

    let executable = helper_executable(workspace_root, target);
    if !executable.is_file() {
        bail!(
            "remote helper build succeeded but artifact is missing: {}",
            executable.display()
        );
    }
    let output = local_helper_path(workspace_root, target);
    let output_dir = output
        .parent()
        .context("remote helper output must have a parent directory")?;
    fs::create_dir_all(output_dir).with_context(|| {
        format!(
            "failed to create remote helper output directory: {}",
            output_dir.display()
        )
    })?;
    fs::copy(&executable, &output).with_context(|| {
        format!(
            "failed to copy remote helper from {} to {}",
            executable.display(),
            output.display()
        )
    })?;
    let bytes = fs::read(&output)
        .with_context(|| format!("failed to read helper artifact: {}", output.display()))?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let checksum = output.with_extension("sha256");
    fs::write(&checksum, format!("{digest}  {HELPER_FILE_NAME}\n")).with_context(|| {
        format!(
            "failed to write remote helper checksum: {}",
            checksum.display()
        )
    })?;
    println!("remote helper artifact: {}", output.display());
    println!("remote helper checksum: {}", checksum.display());
    Ok(())
}

fn linker_env_name(target: &str) -> String {
    format!(
        "CARGO_TARGET_{}_LINKER",
        target.replace('-', "_").to_ascii_uppercase()
    )
}

fn discover_linker(target: &str) -> Result<PathBuf> {
    let env_name = linker_env_name(target);
    if let Some(path) = std::env::var_os(&env_name).filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    let executable = match target {
        "aarch64-unknown-linux-musl" => "aarch64-linux-musl-gcc",
        "x86_64-unknown-linux-musl" => "x86_64-linux-musl-gcc",
        _ => unreachable!("target was validated before linker discovery"),
    };
    which::which(executable).with_context(|| {
        format!(
            concat!(
                "缺少 {} 的链接器 '{}'；请将其加入 PATH 或设置 {}。",
                "若不想分别安装 musl GCC，可安装 Zig 与 cargo-zigbuild：",
                " `winget install zig.zig`、`cargo install cargo-zigbuild --locked`；两者在 PATH 后会自动使用。"
            ),
            target, executable, env_name
        )
    })
}

fn helper_executable(workspace_root: &Path, target: &str) -> PathBuf {
    workspace_root
        .join("target")
        .join(target)
        .join("release")
        .join(HELPER_FILE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_selection_requires_supported_target() {
        let error = selected_targets(&BuildRemoteHelperOptions {
            target: Some("armv7-unknown-linux-musleabihf".to_string()),
            all_targets: false,
        })
        .expect_err("unsupported target");
        assert!(
            error
                .to_string()
                .contains("unsupported remote helper target")
        );
    }

    #[test]
    fn all_targets_is_stable() {
        assert_eq!(
            selected_targets(&BuildRemoteHelperOptions {
                target: None,
                all_targets: true,
            })
            .expect("all targets"),
            SUPPORTED_TARGETS
        );
    }

    #[test]
    fn linker_env_name_uses_cargo_convention() {
        assert_eq!(
            linker_env_name("aarch64-unknown-linux-musl"),
            "CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER"
        );
    }

    #[test]
    fn default_builder_uses_zig_when_both_tools_are_available() {
        assert_eq!(choose_default_builder(true, true), CargoBuilder::Zigbuild);
        assert_eq!(choose_default_builder(true, false), CargoBuilder::Cargo);
        assert_eq!(choose_default_builder(false, true), CargoBuilder::Cargo);
    }
}
