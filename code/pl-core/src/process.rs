use std::process::Stdio;
#[cfg(unix)]
use std::time::Duration;

use process_wrap::tokio::{CommandWrap, KillOnDrop};
use tokio::process::Command as TokioCommand;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[cfg(windows)]
#[derive(Debug)]
struct WindowsBackgroundCreationFlags;

#[cfg(windows)]
impl process_wrap::tokio::CommandWrapper for WindowsBackgroundCreationFlags {
    fn pre_spawn(
        &mut self,
        command: &mut TokioCommand,
        _core: &CommandWrap,
    ) -> std::io::Result<()> {
        use windows::Win32::System::Threading::{
            CREATE_NO_WINDOW as WINDOWS_CREATE_NO_WINDOW, CREATE_SUSPENDED,
        };

        command.creation_flags(WINDOWS_CREATE_NO_WINDOW.0 | CREATE_SUSPENDED.0);
        Ok(())
    }
}

/// 后台子进程的统一配置工厂（全仓唯一来源）。
///
/// GUI 运行时派生 shell、git、MCP server、LSP 等后台子进程时必须通过这里的
/// 配置函数收尾，禁止在调用点自行拼装 flags 或在其他 crate 复制本实现：
/// Windows 上统一使用 `CREATE_NO_WINDOW`，保证 GUI 进程派生的控制台子进程
/// 不弹出新的命令行窗口；Unix 上统一使用独立进程组，便于整树回收。
///
/// 调用约定：先构建 `Command`（program、args、cwd、env、stdio），再调用对应
/// 配置函数，最后 `spawn`/`status`。`tokio` 版本额外启用 `kill_on_drop`。
pub fn configure_background_command(command: &mut TokioCommand) {
    command.kill_on_drop(true);
    #[cfg(windows)]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        command.process_group(0);
    }
}

pub fn configure_background_std_command(command: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

/// 把需要 Windows Job Object 或 Unix process group 的 Tokio command
/// 收口为统一后台进程 owner。
///
/// `process-wrap` 的 Job Object 会重写 creation flags，因此调用方不能先在
/// 原生 [`TokioCommand`] 上配置 flags；本工厂使用 wrapper shim 合并
/// `CREATE_NO_WINDOW` 与 `CREATE_SUSPENDED`，并统一启用 drop 清理。
pub fn wrap_background_command(command: TokioCommand) -> CommandWrap {
    let mut command = CommandWrap::from(command);
    command.wrap(KillOnDrop);
    #[cfg(windows)]
    {
        use process_wrap::tokio::JobObject;

        command.wrap(JobObject);
        // process-wrap 的 JobObject pre_spawn 会覆盖原始 creation flags；项目
        // wrapper 必须最后写入完整 flags，且保留 CREATE_SUSPENDED 供 Job Object
        // 在关联完成后恢复线程。不要改回库内置 CreationFlags + JobObject 顺序。
        command.wrap(WindowsBackgroundCreationFlags);
    }
    #[cfg(unix)]
    {
        use process_wrap::tokio::ProcessGroup;

        command.wrap(ProcessGroup::leader());
    }
    command
}

pub(crate) async fn terminate_process_tree(pid: Option<u32>) {
    let Some(pid) = pid else { return };
    #[cfg(windows)]
    {
        let mut command = TokioCommand::new("taskkill");
        command
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure_background_command(&mut command);
        let _ = command.status().await;
    }
    #[cfg(unix)]
    {
        let group = format!("-{pid}");
        let mut terminate = TokioCommand::new("kill");
        terminate
            .args(["-TERM", "--", &group])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure_background_command(&mut terminate);
        let delivered = terminate
            .status()
            .await
            .map(|status| status.success())
            .unwrap_or(false);
        if delivered {
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        let mut kill = TokioCommand::new("kill");
        kill.args(["-KILL", "--", &group])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure_background_command(&mut kill);
        let _ = kill.status().await;
    }
}

pub(crate) fn terminate_process_tree_sync(pid: Option<u32>) {
    let Some(pid) = pid else { return };
    #[cfg(windows)]
    {
        let mut command = std::process::Command::new("taskkill");
        command
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure_background_std_command(&mut command);
        let _ = command.status();
    }
    #[cfg(unix)]
    {
        let group = format!("-{pid}");
        let mut terminate = std::process::Command::new("kill");
        terminate
            .args(["-TERM", "--", &group])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure_background_std_command(&mut terminate);
        let delivered = terminate
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if delivered {
            std::thread::sleep(Duration::from_secs(2));
        }
        let mut kill = std::process::Command::new("kill");
        kill.args(["-KILL", "--", &group])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure_background_std_command(&mut kill);
        let _ = kill.status();
    }
}

#[cfg(all(test, windows))]
mod tests {
    use std::process::Stdio;

    use super::*;

    const NO_CONSOLE_CHILD: &str = "PURE_TEST_NO_CONSOLE_CHILD";
    const NO_CONSOLE_CHILD_TEST: &str = "process::tests::background_child_has_no_console_window";

    #[test]
    fn background_child_has_no_console_window() {
        if std::env::var_os(NO_CONSOLE_CHILD).is_none() {
            return;
        }

        let console = unsafe { windows::Win32::System::Console::GetConsoleWindow() };
        assert!(
            console.is_invalid(),
            "background child unexpectedly inherited or created a console: {console:?}"
        );
    }

    #[tokio::test]
    async fn wrapped_background_command_preserves_no_console_flag() {
        let mut command = TokioCommand::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", NO_CONSOLE_CHILD_TEST, "--nocapture"])
            .env(NO_CONSOLE_CHILD, "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        let mut command = wrap_background_command(command);
        let mut child = command.spawn().unwrap();
        let status = child.wait().await.unwrap();
        assert!(status.success(), "no-console child failed: {status}");
    }
}
