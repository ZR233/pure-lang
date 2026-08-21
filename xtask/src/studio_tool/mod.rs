use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

use anyhow::Result;

use crate::{paths, process, pubspec_lock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StudioTool {
    Flutter,
    Dart,
}

pub(crate) fn run(tool: StudioTool, args: Vec<OsString>) -> Result<()> {
    let workspace_root = paths::workspace_root()?;
    let app_dir = paths::studio_app_dir(&workspace_root);
    let mut command = studio_command(tool, &args, &app_dir);

    let program = tool.program();
    let display = process::display_command(program, &args);
    let result = process::run_checked(&mut command, &display);
    let canonicalization = pubspec_lock::rewrite_hosted_urls(
        &app_dir.join("pubspec.lock"),
        pubspec_lock::CANONICAL_HOSTED_URL,
    );
    result?;
    canonicalization
}

fn studio_command(tool: StudioTool, args: &[OsString], app_dir: &Path) -> Command {
    let mut command = process::path_command(tool.program(), args);
    command.current_dir(app_dir);
    command
}

impl StudioTool {
    fn program(self) -> &'static str {
        match self {
            Self::Flutter => "flutter",
            Self::Dart => "dart",
        }
    }
}
