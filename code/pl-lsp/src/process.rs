use std::io;

#[cfg(windows)]
use process_wrap::tokio::JobObject;
#[cfg(unix)]
use process_wrap::tokio::ProcessGroup;
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use tokio::process::Command;

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

/// 派生 LSP server / rustup 探测等后台子进程的统一入口。
///
/// 语义与 `pl_core::process::configure_background_command` 等价（Windows
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
