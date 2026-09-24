use anyhow::Result;
use std::ffi::OsString;

mod cli;
mod flutter;
mod manual_gui;
mod paths;
mod process;
mod pubspec_lock;
mod release;
mod remote_helper;
mod rust_bridge;
mod studio_tool;
mod studio_version;
mod sync_skills;

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
            cli::Command::CheckGuiGenerated => flutter::check_gui_generated(),
            cli::Command::VerifyGui => flutter::verify_gui(),
            cli::Command::ManualGui(options) => manual_gui::run(options),
            cli::Command::RunGui(options) => flutter::run_gui(options),
            cli::Command::BuildGui(options) => flutter::build_gui(options),
            cli::Command::ReleaseGui { action } => release::run(action),
            cli::Command::BuildRemoteHelper(options) => remote_helper::build(options),
            cli::Command::SyncSkills => sync_skills::run(),
        },
    }
}
