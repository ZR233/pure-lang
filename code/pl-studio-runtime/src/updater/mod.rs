//! anywork 稳定版更新检查、可信下载与安装边界。

mod client;
mod error;
mod install;
mod manifest;
mod types;

pub use client::StudioUpdater;
pub use error::*;
pub use install::{PreparedStudioUpdate, StudioUpdateCancellation, StudioUpdateLaunch};
pub use types::*;

pub(crate) use client::STUDIO_VERSION;
