use std::io;
use std::pin::Pin;

#[cfg(windows)]
use process_wrap::tokio::JobObject;
#[cfg(unix)]
use process_wrap::tokio::ProcessGroup;
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use tokio::process::Command;

use super::{LspHostProcess, LspHostReader, LspHostWriter};

#[cfg(windows)]
#[derive(Debug)]
struct WindowsBackgroundCreationFlags;

#[cfg(windows)]
impl process_wrap::tokio::CommandWrapper for WindowsBackgroundCreationFlags {
    fn pre_spawn(&mut self, command: &mut Command, _core: &CommandWrap) -> io::Result<()> {
        use windows::Win32::System::Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED};

        command.creation_flags(CREATE_NO_WINDOW.0 | CREATE_SUSPENDED.0);
        Ok(())
    }
}

pub(crate) type ManagedChild = Box<dyn ChildWrapper>;

pub(crate) enum LspChild {
    Local(ManagedChild),
    Hosted(LspHostProcess),
}

impl LspChild {
    pub(crate) fn take_stdin(&mut self) -> Option<LspHostWriter> {
        match self {
            Self::Local(child) => child
                .stdin()
                .take()
                .map(|stdin| Box::pin(stdin) as LspHostWriter),
            Self::Hosted(child) => child.take_stdin(),
        }
    }

    pub(crate) fn take_stdout(&mut self) -> Option<LspHostReader> {
        match self {
            Self::Local(child) => child
                .stdout()
                .take()
                .map(|stdout| Box::pin(stdout) as LspHostReader),
            Self::Hosted(child) => child.take_stdout(),
        }
    }

    pub(crate) fn take_stderr(&mut self) -> Option<LspHostReader> {
        match self {
            Self::Local(child) => child
                .stderr()
                .take()
                .map(|stderr| Box::pin(stderr) as LspHostReader),
            Self::Hosted(child) => child.take_stderr(),
        }
    }

    pub(crate) async fn wait(&mut self) -> Result<(), String> {
        match self {
            Self::Local(child) => child
                .wait()
                .await
                .map(|_| ())
                .map_err(|error| error.to_string()),
            Self::Hosted(child) => child
                .wait()
                .await
                .map(|_| ())
                .map_err(|error| error.to_string()),
        }
    }

    /// 请求终止并等待整棵进程树真实退出。
    ///
    /// `Ok(Some(issue))` 表示已确认回收但请求路径降级；`Ok(None)` 为完全优雅；
    /// `Err` 表示无法确认回收，调用方必须保留 child owner 以便复用/重试。
    pub(crate) async fn terminate_and_wait(&mut self) -> Result<Option<String>, String> {
        match self {
            Self::Local(child) => {
                let kill_issue = match Pin::from(child.kill()).await {
                    Ok(()) => None,
                    Err(error) => Some(format!("LSP process kill request failed: {error}")),
                };
                match child.wait().await {
                    Ok(_status) => Ok(kill_issue),
                    Err(error) => Err(error.to_string()),
                }
            }
            Self::Hosted(child) => child
                .terminate()
                .await
                .map(|()| None)
                .map_err(|error| error.to_string()),
        }
    }
}

/// 派生 LSP server / rustup 探测等后台子进程的统一入口。
///
/// 语义与 `pl_remote_helper::process::configure_background_command` 等价（Windows
/// `CREATE_NO_WINDOW` 不弹窗、进程随宿主回收），并额外通过 Job Object /
/// process group 保证整棵进程树跟随本 crate 退出；因依赖方向
/// （pl-core → pl-lsp）不能复用 pl-core 的工厂，本入口保持为 pl-lsp 内
/// 唯一进程创建点，调用方不得自行拼装 flags。
pub(crate) fn spawn_background(command: Command) -> io::Result<ManagedChild> {
    let mut command = CommandWrap::from(command);
    command.wrap(KillOnDrop);
    #[cfg(windows)]
    {
        command.wrap(JobObject);
        // 与 pl-core 进程工厂保持等价：JobObject 会覆盖 creation flags，
        // 因此必须在最后写入完整的后台进程 flags。
        command.wrap(WindowsBackgroundCreationFlags);
    }
    #[cfg(unix)]
    command.wrap(ProcessGroup::leader());
    command.spawn()
}
