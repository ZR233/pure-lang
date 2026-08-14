use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

use anyhow::Result;

use crate::{paths, process};

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
    process::run_checked(&mut command, &display)
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

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::ffi::OsStr;

    #[test]
    fn flutter_command_preserves_forwarded_arguments() {
        let args = [
            "test",
            "--dart-define=GREETING=你好 world",
            "--",
            "--name",
            "focused test",
        ]
        .map(OsString::from);

        let app_dir = paths::studio_app_dir(Path::new("workspace"));
        let command = studio_command(StudioTool::Flutter, &args, &app_dir);

        assert_tool_command(&command, StudioTool::Flutter, &args, &app_dir);
    }

    #[test]
    fn dart_command_preserves_forwarded_arguments() {
        let args = ["format", "lib", "", "--output=none"].map(OsString::from);

        let app_dir = paths::studio_app_dir(Path::new("workspace"));
        let command = studio_command(StudioTool::Dart, &args, &app_dir);

        assert_tool_command(&command, StudioTool::Dart, &args, &app_dir);
    }

    fn assert_tool_command(
        command: &Command,
        tool: StudioTool,
        expected_args: &[OsString],
        expected_app_dir: &Path,
    ) {
        let actual_args = command.get_args().collect::<Vec<_>>();

        assert_eq!(command.get_current_dir(), Some(expected_app_dir));
        if cfg!(windows) {
            assert_eq!(command.get_program(), OsStr::new("cmd"));
            assert_eq!(
                actual_args[..2],
                [OsStr::new("/c"), OsStr::new(tool.program())]
            );
            assert_eq!(actual_args[2..], *expected_args);
        } else {
            assert_eq!(command.get_program(), OsStr::new(tool.program()));
            assert_eq!(actual_args, expected_args);
        }
    }
}
