//! 更新状态目录页：声明各状态载荷与命令、快照、迁移子模块，并导出稳定入口。

mod available;
mod check_failed;
mod checking;
mod command;
mod disabled;
mod downloading;
mod idle;
mod install_failed;
mod installer_launched;
mod snapshot;
mod transition;
mod up_to_date;
mod verifying;

pub use available::AvailableUpdateState;
pub use check_failed::CheckFailedUpdateState;
pub use checking::CheckingUpdateState;
pub use command::StudioUpdateCommand;
pub use disabled::DisabledUpdateState;
pub use downloading::DownloadingUpdateState;
pub use idle::IdleUpdateState;
pub use install_failed::InstallFailedUpdateState;
pub use installer_launched::InstallerLaunchedUpdateState;
pub use snapshot::{StudioUpdateStateKind, StudioUpdateStateSnapshot};
pub use up_to_date::UpToDateUpdateState;
pub use verifying::VerifyingUpdateState;
