use crate::studio_version::StudioVersionBump;
use anyhow::{Result, anyhow, bail};
use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Command {
    Help(HelpTopic),
    RunGui(RunGuiOptions),
    BuildGui(BuildGuiOptions),
    ReleaseGui(ReleaseGuiOptions),
    BuildRustBridge(BuildRustBridgeOptions),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HelpTopic {
    Global,
    RunGui,
    BuildGui,
    ReleaseGui,
    BuildRustBridge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RunGuiOptions {
    pub(crate) demo: bool,
    pub(crate) demo_fallback: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BuildGuiOptions {
    pub(crate) demo: bool,
    pub(crate) no_clean: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReleaseGuiOptions {
    Prepare { bump: StudioVersionBump },
    Stage { version: String },
    Finalize { version: String },
    Verify { version: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BuildRustBridgeOptions {
    pub(crate) workspace_root: PathBuf,
    pub(crate) configuration: BridgeConfiguration,
    pub(crate) output_dir: PathBuf,
    pub(crate) target_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BridgeConfiguration {
    Debug,
    Profile,
    Release,
}

impl BridgeConfiguration {
    pub(crate) fn profile_dir(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Profile | Self::Release => "release",
        }
    }

    pub(crate) fn uses_release_profile(self) -> bool {
        matches!(self, Self::Profile | Self::Release)
    }
}

pub(crate) fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Command> {
    let mut args: VecDeque<OsString> = args.into_iter().collect();
    let _program = args.pop_front();
    let Some(command) = args.pop_front() else {
        return Ok(Command::Help(HelpTopic::Global));
    };
    let command = into_string(command)?;

    match command.as_str() {
        "-h" | "--help" | "help" => Ok(Command::Help(HelpTopic::Global)),
        "run-gui" => parse_run_gui(args),
        "build-gui" => parse_build_gui(args),
        "release-gui" => parse_release_gui(args),
        "build-rust-bridge" => parse_build_rust_bridge(args),
        _ => bail!(
            "unknown xtask command: {command}\n\n{}",
            help_text(HelpTopic::Global)
        ),
    }
}

pub(crate) fn print_help(topic: HelpTopic) {
    println!("{}", help_text(topic));
}

pub(crate) fn help_text(topic: HelpTopic) -> &'static str {
    match topic {
        HelpTopic::Global => {
            "Usage: cargo xtask <command> [options]\n\nCommands:\n  run-gui             Run the Pure Studio Flutter desktop app.\n  build-gui           Build release artifacts for the current desktop OS.\n  release-gui         Stage, finalize, or verify a Windows stable release.\n  build-rust-bridge   Build the Windows Rust bridge DLL for Flutter CMake.\n\nRun `cargo xtask <command> --help` for command-specific options."
        }
        HelpTopic::RunGui => {
            "Usage: cargo xtask run-gui [--demo] [--demo-fallback]\n\nOptions:\n  --demo           Run with PURE_STUDIO_DEMO=true.\n  --demo-fallback  Retry in demo mode if the native run fails.\n  -h, --help       Print help."
        }
        HelpTopic::BuildGui => {
            "Usage: cargo xtask build-gui [--demo] [--no-clean]\n\nOptions:\n  --demo      Build with PURE_STUDIO_DEMO=true.\n  --no-clean  Keep existing files in dist/pure-studio-flutter-release.\n  -h, --help  Print help."
        }
        HelpTopic::ReleaseGui => {
            "Usage:\n  cargo xtask release-gui prepare --bump <patch|minor|major>\n  cargo xtask release-gui <stage|finalize|verify> --version <x.y.z>\n\nPrepare increments pubspec.yaml and prints typed JSON. Package versions must be stable SemVer and match pubspec.yaml. Signing uses the minisign executable and MINISIGN_SECRET_KEY_FILE for finalize."
        }
        HelpTopic::BuildRustBridge => {
            "Usage: cargo xtask build-rust-bridge --workspace-root <path> --configuration <Debug|Profile|Release> --output-dir <path> [--target-dir <path>]\n\nOptions:\n  --workspace-root <path>              Pure-Lang workspace root.\n  --configuration <Debug|Profile|Release>\n  --output-dir <path>                  Directory that receives pl_studio_bridge.dll.\n  --target-dir <path>                  Optional Cargo target directory.\n  -h, --help                           Print help."
        }
    }
}

fn parse_release_gui(mut args: VecDeque<OsString>) -> Result<Command> {
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        return Ok(Command::Help(HelpTopic::ReleaseGui));
    }
    let action = match args.pop_front().map(into_string).transpose()?.as_deref() {
        Some("prepare") => {
            let bump = parse_required_option(&mut args, "--bump")?;
            let bump = match bump.as_str() {
                "patch" => StudioVersionBump::Patch,
                "minor" => StudioVersionBump::Minor,
                "major" => StudioVersionBump::Major,
                _ => bail!("release bump must be one of patch, minor, or major; got {bump}"),
            };
            ReleaseGuiOptions::Prepare { bump }
        }
        Some("stage") => ReleaseGuiOptions::Stage {
            version: parse_required_option(&mut args, "--version")?,
        },
        Some("finalize") => ReleaseGuiOptions::Finalize {
            version: parse_required_option(&mut args, "--version")?,
        },
        Some("verify") => ReleaseGuiOptions::Verify {
            version: parse_required_option(&mut args, "--version")?,
        },
        Some(other) => bail!("unknown release-gui action: {other}"),
        None => bail!("missing release-gui action"),
    };
    Ok(Command::ReleaseGui(action))
}

fn parse_required_option(args: &mut VecDeque<OsString>, name: &str) -> Result<String> {
    let Some(option) = args.pop_front() else {
        bail!("missing required option {name}");
    };
    let option = into_string(option)?;
    if option != name {
        bail!("unknown release-gui option: {option}");
    }
    let value = next_string(args, name)?;
    if let Some(extra) = args.pop_front() {
        bail!("unknown release-gui option: {}", into_string(extra)?);
    }
    Ok(value)
}

fn parse_run_gui(mut args: VecDeque<OsString>) -> Result<Command> {
    let mut options = RunGuiOptions {
        demo: false,
        demo_fallback: false,
    };
    while let Some(arg) = args.pop_front() {
        match into_string(arg)?.as_str() {
            "-h" | "--help" => return Ok(Command::Help(HelpTopic::RunGui)),
            "--demo" => options.demo = true,
            "--demo-fallback" => options.demo_fallback = true,
            other => bail!(
                "unknown run-gui option: {other}\n\n{}",
                help_text(HelpTopic::RunGui)
            ),
        }
    }
    Ok(Command::RunGui(options))
}

fn parse_build_gui(mut args: VecDeque<OsString>) -> Result<Command> {
    let mut options = BuildGuiOptions {
        demo: false,
        no_clean: false,
    };
    while let Some(arg) = args.pop_front() {
        match into_string(arg)?.as_str() {
            "-h" | "--help" => return Ok(Command::Help(HelpTopic::BuildGui)),
            "--demo" => options.demo = true,
            "--no-clean" => options.no_clean = true,
            other => bail!(
                "unknown build-gui option: {other}\n\n{}",
                help_text(HelpTopic::BuildGui)
            ),
        }
    }
    Ok(Command::BuildGui(options))
}

fn parse_build_rust_bridge(mut args: VecDeque<OsString>) -> Result<Command> {
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        return Ok(Command::Help(HelpTopic::BuildRustBridge));
    }

    let mut workspace_root = None;
    let mut configuration = None;
    let mut output_dir = None;
    let mut target_dir = None;

    while let Some(arg) = args.pop_front() {
        match into_string(arg)?.as_str() {
            "--workspace-root" => workspace_root = Some(next_path(&mut args, "--workspace-root")?),
            "--configuration" => {
                configuration = Some(parse_configuration(next_string(
                    &mut args,
                    "--configuration",
                )?)?);
            }
            "--output-dir" => output_dir = Some(next_path(&mut args, "--output-dir")?),
            "--target-dir" => target_dir = Some(next_path(&mut args, "--target-dir")?),
            other => bail!(
                "unknown build-rust-bridge option: {other}\n\n{}",
                help_text(HelpTopic::BuildRustBridge)
            ),
        }
    }

    Ok(Command::BuildRustBridge(BuildRustBridgeOptions {
        workspace_root: workspace_root
            .ok_or_else(|| anyhow!("missing required option --workspace-root"))?,
        configuration: configuration
            .ok_or_else(|| anyhow!("missing required option --configuration"))?,
        output_dir: output_dir.ok_or_else(|| anyhow!("missing required option --output-dir"))?,
        target_dir,
    }))
}

fn parse_configuration(value: String) -> Result<BridgeConfiguration> {
    match value.as_str() {
        "Debug" => Ok(BridgeConfiguration::Debug),
        "Profile" => Ok(BridgeConfiguration::Profile),
        "Release" => Ok(BridgeConfiguration::Release),
        _ => bail!("configuration must be one of Debug, Profile, or Release; got {value}"),
    }
}

fn next_path(args: &mut VecDeque<OsString>, flag: &str) -> Result<PathBuf> {
    let value = args
        .pop_front()
        .ok_or_else(|| anyhow!("missing value for {flag}"))?;
    Ok(PathBuf::from(value))
}

fn next_string(args: &mut VecDeque<OsString>, flag: &str) -> Result<String> {
    let value = args
        .pop_front()
        .ok_or_else(|| anyhow!("missing value for {flag}"))?;
    into_string(value)
}

fn into_string(value: OsString) -> Result<String> {
    value
        .into_string()
        .map_err(|value| anyhow!("argument is not valid UTF-8: {}", value.to_string_lossy()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn parse_words(words: &[&str]) -> Result<Command> {
        parse(words.iter().map(OsString::from))
    }

    #[test]
    fn parses_run_gui_flags() -> Result<()> {
        assert_eq!(
            parse_words(&["xtask", "run-gui", "--demo", "--demo-fallback"])?,
            Command::RunGui(RunGuiOptions {
                demo: true,
                demo_fallback: true,
            })
        );
        Ok(())
    }

    #[test]
    fn parses_build_gui_flags() -> Result<()> {
        assert_eq!(
            parse_words(&["xtask", "build-gui", "--demo", "--no-clean"])?,
            Command::BuildGui(BuildGuiOptions {
                demo: true,
                no_clean: true,
            })
        );
        Ok(())
    }

    #[test]
    fn parses_build_rust_bridge_options() -> Result<()> {
        assert_eq!(
            parse_words(&[
                "xtask",
                "build-rust-bridge",
                "--workspace-root",
                "repo",
                "--configuration",
                "Release",
                "--output-dir",
                "out",
                "--target-dir",
                "target",
            ])?,
            Command::BuildRustBridge(BuildRustBridgeOptions {
                workspace_root: PathBuf::from("repo"),
                configuration: BridgeConfiguration::Release,
                output_dir: PathBuf::from("out"),
                target_dir: Some(PathBuf::from("target")),
            })
        );
        Ok(())
    }

    #[test]
    fn parses_release_gui_options() -> Result<()> {
        assert_eq!(
            parse_words(&["xtask", "release-gui", "finalize", "--version", "1.2.3",])?,
            Command::ReleaseGui(ReleaseGuiOptions::Finalize {
                version: "1.2.3".to_string(),
            })
        );
        assert_eq!(
            parse_words(&["xtask", "release-gui", "prepare", "--bump", "minor",])?,
            Command::ReleaseGui(ReleaseGuiOptions::Prepare {
                bump: StudioVersionBump::Minor,
            })
        );
        Ok(())
    }

    #[test]
    fn rejects_invalid_release_prepare_options() {
        assert!(parse_words(&["xtask", "release-gui", "prepare", "--bump", "feature"]).is_err());
        assert!(parse_words(&["xtask", "release-gui", "prepare"]).is_err());
        assert!(
            parse_words(&[
                "xtask",
                "release-gui",
                "prepare",
                "--bump",
                "patch",
                "--version",
                "1.2.3",
            ])
            .is_err()
        );
    }

    #[test]
    fn prints_subcommand_help() -> Result<()> {
        assert_eq!(
            parse_words(&["xtask", "build-gui", "--help"])?,
            Command::Help(HelpTopic::BuildGui)
        );
        Ok(())
    }
}
