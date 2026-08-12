use std::io;

#[cfg(unix)]
use process_wrap::tokio::ProcessGroup;
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
#[cfg(windows)]
use process_wrap::tokio::{CreationFlags, JobObject};
use tokio::process::Command;

#[cfg(windows)]
const CREATE_NO_WINDOW: windows::Win32::System::Threading::PROCESS_CREATION_FLAGS =
    windows::Win32::System::Threading::CREATE_NO_WINDOW;

pub(crate) type ManagedChild = Box<dyn ChildWrapper>;

pub(crate) fn spawn_background(command: Command) -> io::Result<ManagedChild> {
    let mut command = CommandWrap::from(command);
    command.wrap(KillOnDrop);
    #[cfg(windows)]
    {
        command.wrap(CreationFlags(CREATE_NO_WINDOW));
        command.wrap(JobObject);
    }
    #[cfg(unix)]
    command.wrap(ProcessGroup::leader());
    command.spawn()
}
