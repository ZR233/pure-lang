use anyhow::Result;
use std::ffi::OsString;

mod cli;
mod flutter;
mod paths;
mod process;
mod pubspec_lock;
mod release;
mod rust_bridge;
mod studio_tool;
mod studio_version;

pub fn run(args: impl IntoIterator<Item = OsString>) -> Result<()> {
    match cli::parse(args)? {
        cli::ParseOutcome::Display(output) => {
            print!("{output}");
            Ok(())
        }
        cli::ParseOutcome::Run(command) => match command {
            cli::Command::Flutter(options) => {
                studio_tool::run(studio_tool::StudioTool::Flutter, options.args)
            }
            cli::Command::Dart(options) => {
                studio_tool::run(studio_tool::StudioTool::Dart, options.args)
            }
            cli::Command::GenerateGui => flutter::generate_gui(),
            cli::Command::VerifyGui(options) => flutter::verify_gui(options),
            cli::Command::RunGui(options) => flutter::run_gui(options),
            cli::Command::BuildGui(options) => flutter::build_gui(options),
            cli::Command::ReleaseGui { action } => release::run(action),
            cli::Command::BuildRustBridge(options) => rust_bridge::build(options),
        },
    }
}
