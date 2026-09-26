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
    /// Run Flutter from the anywork app directory.
    Flutter(ToolOptions),
    /// Run Dart from the anywork app directory.
    Dart(ToolOptions),
    /// Regenerate Riverpod, Freezed, l10n, and FRB bindings.
    GenerateGui,
    /// Regenerate GUI sources and fail when generated files are not committed.
    CheckGuiGenerated,
    /// Check generated sources, formatting, and Flutter analysis.
    VerifyGui,
    /// Start an isolated native GUI with a local provider fixture for manual review.
    ManualGui(ManualGuiOptions),
    /// Run the anywork desktop app.
    RunGui(RunGuiOptions),
    /// Build release artifacts for the current desktop OS.
    BuildGui(BuildGuiOptions),
    /// Stage, finalize, or verify a Windows stable release.
    ReleaseGui {
        #[command(subcommand)]
        action: ReleaseGuiOptions,
    },
    /// Cross-compile the minimal Linux SSH remote helper.
    BuildRemoteHelper(BuildRemoteHelperOptions),
    /// Refresh bundled upstream preset Skills inside pl-studio-runtime.
    SyncSkills,
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

#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub(crate) struct ManualGuiOptions {
    /// Run the regular, stress, single-item long-body stress (`stress-body`,
    /// `stress-body-large`), isolated call-statistics, realtime, paused
    /// history-writer, or history-fault retry/resume acceptance journey.
    #[arg(long, default_value = "gui", value_parser = pl_provider_fixture::GUI_SCENARIOS)]
    pub(crate) scenario: String,
    /// Directory for sanitized evidence (defaults to target/manual-gui/<timestamp>-<pid>).
    #[arg(long, value_name = "DIR")]
    pub(crate) output: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub(crate) struct RunGuiOptions {
    /// Run with ANYWORK_DEMO=true.
    #[arg(long)]
    pub(crate) demo: bool,
    /// Enable Flutter Driver through test_driver/driver_main.dart.
    #[arg(long)]
    pub(crate) driver: bool,
    /// Run the Driver in profile/AOT mode for representative frame timings.
    #[arg(long, requires = "driver")]
    pub(crate) profile: bool,
    /// Deterministic file-picker result exposed only to the Driver build.
    #[arg(long, value_name = "PATH", requires = "driver")]
    pub(crate) driver_attachment: Option<std::path::PathBuf>,
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
    /// Build with ANYWORK_DEMO=true.
    #[arg(long)]
    pub(crate) demo: bool,
    /// Keep existing files in dist/anywork-release.
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
        /// Stable SemVer matching code/anywork/pubspec.yaml.
        #[arg(long)]
        version: String,
    },
    /// Sign and finalize staged release artifacts.
    Finalize {
        /// Stable SemVer matching code/anywork/pubspec.yaml.
        #[arg(long)]
        version: String,
    },
    /// Verify finalized release artifacts.
    Verify {
        /// Stable SemVer matching code/anywork/pubspec.yaml.
        #[arg(long)]
        version: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub(crate) struct BuildRemoteHelperOptions {
    /// Rust target triple for one helper artifact.
    #[arg(long, value_name = "TARGET", conflicts_with = "all_targets")]
    pub(crate) target: Option<String>,
    /// Build every supported helper target.
    #[arg(long, conflicts_with = "target")]
    pub(crate) all_targets: bool,
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
