use std::ffi::OsString;

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
    /// Build and run the isolated GUI acceptance tool (arguments forwarded).
    ManualGui(ToolOptions),
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
pub(crate) struct RunGuiOptions {
    /// Run the existing release bundle without building or resolving dependencies.
    #[arg(long, conflicts_with_all = ["demo", "driver", "profile"])]
    pub(crate) release: bool,
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
    /// Build the debug/profile Driver bundle and launch the native artifact
    /// directly so this process is its real parent.
    ///
    /// The platform launch tool always reports exit code 0 for the app it
    /// started (`resident_runner.appFinished`), so a native process that exited
    /// non-zero is otherwise indistinguishable from a clean one. Direct launch
    /// makes the true wait status observable and keeps the VM service available
    /// through engine switches. Driver-only.
    #[arg(long, requires = "driver")]
    pub(crate) native_launch: bool,
    /// Write a structured JSON exit report for the `--native-launch` process.
    #[arg(long, value_name = "PATH", requires = "native_launch")]
    pub(crate) native_exit_report: Option<std::path::PathBuf>,
    /// Driver entrypoint to build and launch with `--native-launch`.
    ///
    /// Defaults to `test_driver/driver_main.dart`. The shutdown acceptance
    /// selects a dedicated Driver-only fault entrypoint here; the path is
    /// resolved relative to the anywork app directory (or used as-is when
    /// absolute). It must be a real file and is passed to the Flutter build as
    /// its `-t` target.
    #[arg(long, value_name = "PATH", requires = "native_launch")]
    pub(crate) native_launch_driver_target: Option<std::path::PathBuf>,
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
    if let Some(command) = parse_forwarded_command(&args) {
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

/// Matches subcommands whose whole argument tail is another program's CLI.
///
/// Forwarding before clap keeps `--help` and every validation with the program
/// that actually defines the options, so xtask never keeps a second copy.
fn parse_forwarded_command(args: &[OsString]) -> Option<Command> {
    let forwarded_args = || args.iter().skip(2).cloned().collect();
    match args.get(1).and_then(|arg| arg.to_str()) {
        Some("flutter") => Some(Command::Flutter(ToolOptions {
            args: forwarded_args(),
        })),
        Some("dart") => Some(Command::Dart(ToolOptions {
            args: forwarded_args(),
        })),
        Some("manual-gui") => Some(Command::ManualGui(ToolOptions {
            args: forwarded_args(),
        })),
        _ => None,
    }
}
