use thiserror::Error;

/// 更新失败的稳定分类，Bridge 使用该分类向 UI 暴露可本地化错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StudioUpdateErrorCode {
    Network,
    InvalidManifest,
    UnsupportedPlatform,
    DownloadTooLarge,
    DownloadIncomplete,
    HashMismatch,
    SignatureInvalid,
    RuntimeBusy,
    InstallInProgress,
    Cancelled,
    CancellationTooLate,
    InstallerLaunchFailed,
    Io,
}

impl StudioUpdateErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Network => "network",
            Self::InvalidManifest => "invalidManifest",
            Self::UnsupportedPlatform => "unsupportedPlatform",
            Self::DownloadTooLarge => "downloadTooLarge",
            Self::DownloadIncomplete => "downloadIncomplete",
            Self::HashMismatch => "hashMismatch",
            Self::SignatureInvalid => "signatureInvalid",
            Self::RuntimeBusy => "runtimeBusy",
            Self::InstallInProgress => "installInProgress",
            Self::Cancelled => "cancelled",
            Self::CancellationTooLate => "cancellationTooLate",
            Self::InstallerLaunchFailed => "installerLaunchFailed",
            Self::Io => "io",
        }
    }
}

/// 更新协议、网络、验证或安装错误。
#[derive(Debug, Error)]
#[error("{message}")]
pub struct StudioUpdateError {
    code: StudioUpdateErrorCode,
    message: String,
}

impl StudioUpdateError {
    pub fn new(code: StudioUpdateErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn code(&self) -> StudioUpdateErrorCode {
        self.code
    }
}
