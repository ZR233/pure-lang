use serde::{Deserialize, Serialize};

use crate::StudioUpdate;

/// 已验证并持有文件租约，等待退出或用户主动重启安装。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadyUpdateState {
    pub(super) revision: u64,
    pub(super) ready_at: i64,
    pub(super) update: StudioUpdate,
}

impl ReadyUpdateState {
    pub const fn ready_at(&self) -> i64 {
        self.ready_at
    }

    pub const fn update(&self) -> &StudioUpdate {
        &self.update
    }
}
