use std::ffi::OsString;
use std::path::PathBuf;

use anyhow::Result;
use clap::error::ErrorKind;
use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "cargo xtask",
    bin_name = "cargo xtask",
    about = "Pure-Lang workspace development tasks",
    subcommand_required = true,
    arg_required_else_help = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub(crate) enum Command {
    /// Run Flutter from the Pure Studio app directory.
    Flutter(ToolOptions),
    /// Run Dart from the Pure Studio app directory.
    Dart(ToolOptions),
    /// Regenerate Riverpod, Freezed, l10n, and FRB bindings.
    GenerateGui,
    /// Regenerate GUI sources and fail when generated files are not committed.
    CheckGuiGenerated,
    /// Generate, analyze, and test the Pure Studio desktop app.
    VerifyGui(VerifyGuiOptions),
    /// Run the Pure Studio desktop app.
    RunGui(RunGuiOptions),
    /// Build release artifacts for the current desktop OS.
    BuildGui(BuildGuiOptions),
    /// Stage, finalize, or verify a Windows stable release.
    ReleaseGui {
        #[command(subcommand)]
        action: ReleaseGuiOptions,
    },
    /// Build and copy Windows Rust bridge artifacts.
    BuildRustBridge(BuildRustBridgeOptions),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ParseOutcome {
    Run(Command),
    Display(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Args)]
#[command(trailing_var_arg = true, disable_help_flag = true)]
pub(crate) struct ToolOptions {
    /// Arguments forwarded to the tool.
    #[arg(value_name = "ARGS", allow_hyphen_values = true)]
    pub(crate) args: Vec<OsString>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Args)]
pub(crate) struct VerifyGuiOptions {
    /// Run the Windows Flutter integration test through flutter drive.
    #[arg(long)]
    pub(crate) integration: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Args)]
pub(crate) struct RunGuiOptions {
    /// Run with PURE_STUDIO_DEMO=true.
    #[arg(long)]
    pub(crate) demo: bool,
    /// Enable Flutter Driver through test_driver/driver_main.dart.
    #[arg(long)]
    pub(crate) driver: bool,
    /// Override RUST_LOG with a process-wide tracing level.
    #[arg(long, value_enum, value_name = "LEVEL")]
    pub(crate) log_level: Option<LogLevel>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Args)]
pub(crate) struct BuildGuiOptions {
    /// Build with PURE_STUDIO_DEMO=true.
    #[arg(long)]
    pub(crate) demo: bool,
    /// Keep existing files in dist/pure-studio-release.
    #[arg(long)]
    pub(crate) no_clean: bool,
    /// Fail when refreshed generated GUI sources differ from Git.
    #[arg(long)]
    pub(crate) check_generated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub(crate) enum ReleaseGuiOptions {
    /// Prepare a release staging directory.
    Stage {
        /// Stable SemVer matching code/pure-studio/pubspec.yaml.
        #[arg(long)]
        version: String,
    },
    /// Sign and finalize staged release artifacts.
    Finalize {
        /// Stable SemVer matching code/pure-studio/pubspec.yaml.
        #[arg(long)]
        version: String,
    },
    /// Verify finalized release artifacts.
    Verify {
        /// Stable SemVer matching code/pure-studio/pubspec.yaml.
        #[arg(long)]
        version: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub(crate) struct BuildRustBridgeOptions {
    /// Pure-Lang workspace root.
    #[arg(long)]
    pub(crate) workspace_root: PathBuf,
    /// Cargo bridge build configuration.
    #[arg(long, value_enum)]
    pub(crate) configuration: BridgeConfiguration,
    /// Directory that receives bridge DLL/PDB artifacts.
    #[arg(long)]
    pub(crate) output_dir: PathBuf,
    /// Optional Cargo target directory.
    #[arg(long)]
    pub(crate) target_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum BridgeConfiguration {
    #[value(name = "Debug")]
    Debug,
    #[value(name = "Profile")]
    Profile,
    #[value(name = "Release")]
    Release,
}

impl BridgeConfiguration {
    pub(crate) fn uses_release_profile(self) -> bool {
        matches!(self, Self::Profile | Self::Release)
    }
}

pub(crate) fn parse(args: impl IntoIterator<Item = OsString>) -> Result<ParseOutcome> {
    let args = args.into_iter().collect::<Vec<_>>();
    if let Some(command) = parse_studio_tool(&args) {
        return Ok(ParseOutcome::Run(command));
    }

    match Cli::try_parse_from(args) {
        Ok(cli) => Ok(ParseOutcome::Run(cli.command)),
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            Ok(ParseOutcome::Display(error.to_string()))
        }
        Err(error) => Err(error.into()),
    }
}

fn parse_studio_tool(args: &[OsString]) -> Option<Command> {
    let forwarded_args = || args.iter().skip(2).cloned().collect();
    match args.get(1).and_then(|arg| arg.to_str()) {
        Some("flutter") => Some(Command::Flutter(ToolOptions {
            args: forwarded_args(),
        })),
        Some("dart") => Some(Command::Dart(ToolOptions {
            args: forwarded_args(),
        })),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn run_gui_options_reach_the_runtime_command() -> Result<()> {
        let outcome = parse(
            [
                "xtask",
                "run-gui",
                "--demo",
                "--driver",
                "--log-level",
                "trace",
            ]
            .map(OsString::from),
        )?;

        assert_eq!(
            outcome,
            ParseOutcome::Run(Command::RunGui(RunGuiOptions {
                demo: true,
                driver: true,
                log_level: Some(LogLevel::Trace),
            }))
        );
        Ok(())
    }

    #[test]
    fn flutter_arguments_reach_the_runtime_command_unchanged() -> Result<()> {
        let outcome = parse(
            [
                "xtask",
                "flutter",
                "test",
                "--dart-define=GREETING=你好 world",
                "--",
                "--name",
                "focused test",
            ]
            .map(OsString::from),
        )?;

        assert_eq!(
            outcome,
            ParseOutcome::Run(Command::Flutter(ToolOptions {
                args: [
                    "test",
                    "--dart-define=GREETING=你好 world",
                    "--",
                    "--name",
                    "focused test",
                ]
                .map(OsString::from)
                .into(),
            }))
        );
        Ok(())
    }

    #[test]
    fn help_is_returned_as_successful_output() -> Result<()> {
        let outcome = parse(["xtask", "--help"].map(OsString::from))?;

        let ParseOutcome::Display(help) = outcome else {
            anyhow::bail!("help was not rendered");
        };
        assert!(help.contains("Pure-Lang workspace development tasks"));
        Ok(())
    }

    #[test]
    fn generated_source_check_has_a_dedicated_command() -> Result<()> {
        assert_eq!(
            parse(["xtask", "check-gui-generated"].map(OsString::from))?,
            ParseOutcome::Run(Command::CheckGuiGenerated)
        );
        Ok(())
    }
}
