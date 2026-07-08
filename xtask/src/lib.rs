use anyhow::Result;
use std::ffi::OsString;

mod cli;
mod flutter;
mod paths;
mod process;
mod pubspec_lock;
mod rust_bridge;

pub fn run(args: impl IntoIterator<Item = OsString>) -> Result<()> {
    match cli::parse(args)? {
        cli::Command::Help(topic) => {
            cli::print_help(topic);
            Ok(())
        }
        cli::Command::RunGui(options) => flutter::run_gui(options),
        cli::Command::BuildGui(options) => flutter::build_gui(options),
        cli::Command::BuildRustBridge(options) => rust_bridge::build(options),
    }
}
