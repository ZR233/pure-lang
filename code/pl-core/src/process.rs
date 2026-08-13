use std::process::Stdio;
#[cfg(unix)]
use std::time::Duration;

use tokio::process::Command as TokioCommand;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

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
