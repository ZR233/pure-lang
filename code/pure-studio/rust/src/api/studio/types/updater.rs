/// Studio 安装进度事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeStudioUpdateEventDto {
    Started { total: u64 },
    Progress { downloaded: u64, total: u64 },
    Verifying,
    InstallerLaunched,
    Failed { code: String, message: String },
}
