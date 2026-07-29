//! Pure Studio 稳定版更新检查、可信下载与安装边界。

mod client;
mod error;
mod install;
mod manifest;
mod types;

pub use client::StudioUpdater;
pub use error::{StudioUpdateError, StudioUpdateErrorCode};
pub use install::StudioUpdateCancellation;
pub use types::{StudioUpdate, StudioUpdateAsset, StudioUpdateCheck, StudioUpdateEvent};
